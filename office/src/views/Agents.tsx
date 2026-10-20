import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { ApiError, api, type PaseoSessionSummary } from "../api";
import { StreamStateBadge } from "../components/StreamStateBadge";
import {
  formatTimestamp,
  fullTimestamp,
  isLiveSession,
  isOrphanedSession,
  SESSION_PROVIDERS,
  SESSION_STATUSES,
  shortId,
} from "../paseoView";
import { useEventStream } from "../useEventStream";

/**
 * Agents view: the read-only Core projection of real durable Paseo session
 * bindings. Every field comes from the binding row and its owning Run; the
 * view never invents providers, activity or lifecycle state.
 *
 * TRUSTED_LOCAL is owner-authorized local execution with broad host access.
 * It is not isolation-certified and this view never labels it as such.
 */
export function Agents() {
  const [sessions, setSessions] = useState<PaseoSessionSummary[] | null>(null);
  const [status, setStatus] = useState("");
  const [provider, setProvider] = useState("");
  const [cursor, setCursor] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastLoaded, setLastLoaded] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const cursorRef = useRef<number | null>(null);
  const requestId = useRef(0);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      requestId.current += 1;
    };
  }, []);

  const load = useCallback(async () => {
    const id = ++requestId.current;
    setLoading(true);
    try {
      const page = await api.listAllSessions({
        status: status || undefined,
        provider: provider || undefined,
        limit: 200,
      });
      if (!alive.current || id !== requestId.current) {
        return;
      }
      setSessions(page.sessions);
      setError(null);
      setLastLoaded(new Date().toISOString());
      if (cursorRef.current === null) {
        cursorRef.current = page.eventCursor;
        setCursor(page.eventCursor);
      }
    } catch (e) {
      if (!alive.current || id !== requestId.current) {
        return;
      }
      setError(
        e instanceof ApiError ? e.message : "Failed to load Paseo sessions.",
      );
    } finally {
      if (alive.current && id === requestId.current) {
        setLoading(false);
      }
    }
  }, [status, provider]);

  useEffect(() => {
    void load();
  }, [load]);

  const relevantEvent = useCallback(
    (eventType: string) =>
      eventType.startsWith("PaseoSession") ||
      eventType.startsWith("Run") ||
      eventType.startsWith("Worker"),
    [],
  );

  const { events: live, state } = useEventStream(
    cursor ?? 0,
    (event) => relevantEvent(event.eventType),
    { enabled: cursor !== null },
  );

  // Refresh the snapshot after relevant committed events. The durable
  // ledger is authoritative; this fetch only re-projects it.
  useEffect(() => {
    if (live.length === 0) {
      return;
    }
    const timer = setTimeout(() => void load(), 200);
    return () => clearTimeout(timer);
  }, [live.length, load]);

  // Poll only while the live stream is not connected; a connected stream
  // already wakes the snapshot above on every relevant event.
  useEffect(() => {
    if (state === "live") {
      return;
    }
    const timer = setInterval(() => void load(), 5000);
    return () => clearInterval(timer);
  }, [state, load]);

  useEffect(() => {
    const onVisibility = () => {
      if (document.visibilityState === "visible") {
        void load();
      }
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, [load]);

  // Before the first successful snapshot there is no stream cursor, so the
  // hook is idle. Surface an explicit unavailable state instead of leaving
  // the badge at "Connecting" forever when the snapshot API is failing.
  const displayState = cursor === null && error ? "error" : state;
  const active = sessions?.filter((s) => isLiveSession(s.status)) ?? [];
  const history = sessions?.filter((s) => !isLiveSession(s.status)) ?? [];
  const orphaned = history.filter((s) => isOrphanedSession(s.status)).length;
  const filtered = Boolean(status || provider);

  return (
    <div>
      <div className="view-head">
        <div>
          <h2>Agents</h2>
          <p className="muted">
            Real Paseo session bindings recorded by Core, with the Factory Run,
            Attempt and Worker that own each session.
          </p>
        </div>
        <StreamStateBadge state={displayState} />
      </div>

      <p className="notice">
        TRUSTED_LOCAL means owner-authorized local execution with broad host
        access. It is not a sandbox, an isolation certification, or a SEC-01
        PASS.
      </p>

      <div className="filters">
        <label>
          Status
          <select
            value={status}
            onChange={(e) => setStatus(e.target.value)}
            aria-label="Filter sessions by status"
          >
            <option value="">All statuses</option>
            {SESSION_STATUSES.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </label>
        <label>
          Provider
          <select
            value={provider}
            onChange={(e) => setProvider(e.target.value)}
            aria-label="Filter sessions by provider"
          >
            <option value="">All providers</option>
            {SESSION_PROVIDERS.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </label>
        <button type="button" onClick={() => void load()} disabled={loading}>
          {loading ? "Refreshing…" : "Refresh"}
        </button>
        <span className="muted">
          {sessions === null
            ? "loading…"
            : `${sessions.length} binding${sessions.length === 1 ? "" : "s"}${
                orphaned > 0 ? ` · ${orphaned} orphaned` : ""
              }`}
        </span>
      </div>

      {error && <p className="error">{error}</p>}
      {displayState !== "live" && sessions !== null && (
        <p className="warning">
          Showing the last snapshot loaded{" "}
          {lastLoaded ? formatTimestamp(lastLoaded) : "earlier"}. A disconnected
          view is not evidence that an agent stopped.
        </p>
      )}

      {sessions === null && !error && (
        <p className="muted">Loading Paseo sessions…</p>
      )}
      {sessions !== null && sessions.length === 0 && (
        <p className="muted">
          {filtered
            ? "No sessions match these filters."
            : "No Paseo agent sessions have been recorded yet. Sessions appear after a trusted-local paseo-task Run reaches session intent."}
        </p>
      )}

      {sessions !== null && sessions.length === 200 && (
        <p className="muted">
          Showing the first 200 bindings (live first, then newest). Narrow the
          filters or open a Run for older history.
        </p>
      )}

      {active.length > 0 && (
        <>
          <h3>Active now ({active.length})</h3>
          <SessionGrid sessions={active} />
        </>
      )}

      {history.length > 0 && (
        <>
          <h3>Recorded history ({history.length})</h3>
          <SessionGrid sessions={history} />
        </>
      )}
    </div>
  );
}

function SessionGrid({ sessions }: { sessions: PaseoSessionSummary[] }) {
  return (
    <div className="session-grid">
      {sessions.map((session) => (
        <SessionCard key={session.attemptId} session={session} />
      ))}
    </div>
  );
}

function SessionCard({ session }: { session: PaseoSessionSummary }) {
  const updated = fullTimestamp(session.updatedAt);
  return (
    <article className={`session-card session-${session.status}`}>
      <header className="session-card-head">
        <div>
          <strong>{session.provider}</strong>
          <span className="muted"> / </span>
          <span>{session.model}</span>
        </div>
        <span className={`pill ${session.status}`}>{session.status}</span>
      </header>
      <p className="session-route">
        <span className="pill trusted-local">TRUSTED_LOCAL</span>{" "}
        <span className="muted">Paseo mode</span> <code>{session.mode}</code>
      </p>
      <dl className="session-meta">
        <dt>Session</dt>
        <dd>
          {session.sessionId ? (
            <code title={session.sessionId}>
              {shortId(session.sessionId, 18)}
            </code>
          ) : (
            <span className="muted">not bound yet</span>
          )}
        </dd>
        <dt>Run</dt>
        <dd>
          <Link to={`/runs/${session.runId}`}>
            <code>{shortId(session.runId)}</code>
          </Link>{" "}
          <span className={`pill ${session.runStatus}`}>
            {session.runStatus}
          </span>
        </dd>
        <dt>Attempt</dt>
        <dd>
          seq {session.seq} ·{" "}
          <code title={session.attemptId}>{shortId(session.attemptId)}</code> ·
          generation {session.leaseGeneration}
        </dd>
        <dt>Worker</dt>
        <dd>
          <Link to="/workers">
            <code>{session.workerId}</code>
          </Link>
        </dd>
        <dt>Workspace</dt>
        <dd>
          {session.workspaceId ? (
            <code title={session.workspaceId}>
              {shortId(session.workspaceId, 18)}
            </code>
          ) : (
            <span className="muted">not bound yet</span>
          )}
        </dd>
        <dt>Created</dt>
        <dd className="muted">
          <time dateTime={fullTimestamp(session.createdAt)}>
            {formatTimestamp(session.createdAt)}
          </time>
        </dd>
        <dt>Updated</dt>
        <dd className="muted">
          <time dateTime={updated}>{formatTimestamp(session.updatedAt)}</time>
        </dd>
      </dl>
      {session.error && <p className="error">{session.error}</p>}
      {isOrphanedSession(session.status) && (
        <p className="warning">
          External state unconfirmed: the Paseo session may still exist. This is
          not proof that it is running, and cleanup may be required.
        </p>
      )}
    </article>
  );
}
