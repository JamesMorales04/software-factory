import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import {
  ApiError,
  type Attempt,
  api,
  type FactoryEvent,
  type PaseoSession,
  type Run,
} from "../api";
import { StreamStateBadge } from "../components/StreamStateBadge";
import { presentEvent } from "../eventPresentation";
import {
  exitLabel,
  formatTimestamp,
  fullTimestamp,
  numberField,
  shortId,
  stringField,
} from "../paseoView";
import { useEventStream } from "../useEventStream";

export function RunDetail() {
  const { id } = useParams<{ id: string }>();
  const [run, setRun] = useState<Run | null>(null);
  const [attempts, setAttempts] = useState<Attempt[]>([]);
  const [sessions, setSessions] = useState<PaseoSession[]>([]);
  const [ledger, setLedger] = useState<FactoryEvent[]>([]);
  const [cursor, setCursor] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastLoaded, setLastLoaded] = useState<string | null>(null);
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
    if (!id) {
      return;
    }
    const request = ++requestId.current;
    try {
      const [nextRun, nextAttempts, nextSessions, nextLedger] =
        await Promise.all([
          api.getRun(id),
          api.listAttempts(id),
          api.listSessions(id),
          api.listRunEvents(id),
        ]);
      if (!alive.current || request !== requestId.current) {
        return;
      }
      setRun(nextRun);
      setAttempts(nextAttempts);
      setSessions(nextSessions);
      setLedger(nextLedger);
      setError(null);
      setLastLoaded(new Date().toISOString());
      setCursor((previous) =>
        previous === null
          ? Math.max(0, ...nextLedger.map((event) => event.globalSeq))
          : previous,
      );
    } catch (e) {
      if (!alive.current || request !== requestId.current) {
        return;
      }
      setError(e instanceof ApiError ? e.message : "Failed to load Run.");
    }
  }, [id]);

  useEffect(() => {
    requestId.current += 1;
    setRun(null);
    setAttempts([]);
    setSessions([]);
    setLedger([]);
    setCursor(null);
    setError(null);
    void load();
  }, [load]);

  const {
    events: live,
    state,
    lastMatchedSeq,
  } = useEventStream(
    cursor ?? 0,
    (event) => event.aggregateType === "run" && event.aggregateId === id,
    { enabled: cursor !== null && Boolean(id), maxEvents: 500 },
  );

  // Re-read the Core snapshot after committed Run/Paseo lifecycle events.
  // The ledger is the continuity source; this fetch only re-projects it.
  useEffect(() => {
    if (lastMatchedSeq <= (cursor ?? 0)) {
      return;
    }
    const timer = setTimeout(() => void load(), 200);
    return () => clearTimeout(timer);
  }, [lastMatchedSeq, cursor, load]);

  // The initial snapshot and the ledger head are separate reads. Refresh
  // once when the stream first becomes live (and after each reconnect) so
  // no snapshot can stay stale across that boundary.
  const wasLive = useRef(false);
  useEffect(() => {
    if (state === "live" && !wasLive.current) {
      wasLive.current = true;
      void load();
    } else if (state !== "live") {
      wasLive.current = false;
    }
  }, [state, load]);

  // Fallback snapshot refresh only while SSE is unavailable. A connected
  // stream already refreshes on each committed event.
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

  if (error && !run) {
    return <p className="error">{error}</p>;
  }
  if (!run) {
    return <p className="muted">Loading…</p>;
  }

  const seen = new Set(ledger.map((event) => event.globalSeq));
  const timeline = [
    ...ledger,
    ...live.filter(
      (event) =>
        event.aggregateType === "run" &&
        event.aggregateId === id &&
        !seen.has(event.globalSeq),
    ),
  ].sort((a, b) => a.globalSeq - b.globalSeq);

  return (
    <div>
      <div className="view-head">
        <h2>
          Run <code>{shortId(run.id)}</code>{" "}
          <span className={`pill ${run.status}`}>{run.status}</span>
        </h2>
        <StreamStateBadge state={state} />
      </div>
      {error && <p className="error">{error}</p>}
      {state !== "live" && (
        <p className="warning">
          Showing the last snapshot loaded{" "}
          {lastLoaded ? formatTimestamp(lastLoaded) : "earlier"}. A disconnected
          view is not evidence that an agent stopped.
        </p>
      )}

      <dl className="detail">
        <dt>ID</dt>
        <dd>
          <code>{run.id}</code>
        </dd>
        <dt>Workload</dt>
        <dd>
          <code>{run.workloadKind}</code>{" "}
          {run.executionMode === "trusted-local" ? (
            <span className="pill trusted-local">TRUSTED_LOCAL</span>
          ) : (
            <span className="pill">{run.executionMode || "restricted"}</span>
          )}
        </dd>
        {run.workloadKind === "paseo-task" && (
          <>
            <dt>Requested route</dt>
            <dd>
              <strong>{run.provider}</strong> / {run.model} ·{" "}
              <code>{run.paseoMode}</code>
            </dd>
          </>
        )}
        <dt>Attempts</dt>
        <dd>
          {run.attemptCount}/{run.maxAttempts} (timeout {run.timeoutSecs}s)
        </dd>
        <dt>Lease</dt>
        <dd className="muted">
          {run.leaseOwner
            ? `${run.leaseOwner} · generation ${run.leaseGeneration}`
            : "none"}
        </dd>
        {run.error && (
          <>
            <dt>Run error</dt>
            <dd className="error">{run.error}</dd>
          </>
        )}
        <dt>Created</dt>
        <dd className="muted">{formatTimestamp(run.createdAt)}</dd>
        <dt>Updated</dt>
        <dd className="muted">{formatTimestamp(run.updatedAt)}</dd>
      </dl>

      <h3>Attempts ({attempts.length})</h3>
      {attempts.length === 0 ? (
        <p className="muted">No Attempts granted yet.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Seq</th>
              <th>Worker</th>
              <th>Generation</th>
              <th>Status</th>
              <th>Heartbeats</th>
              <th>Session</th>
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            {attempts.map((attempt) => {
              const session = sessions.find(
                (candidate) => candidate.attemptId === attempt.id,
              );
              return (
                <tr key={attempt.id} id={`attempt-${attempt.id}`}>
                  <td>{attempt.seq}</td>
                  <td>
                    <Link to="/workers">
                      <code>{attempt.workerId}</code>
                    </Link>
                  </td>
                  <td>{attempt.leaseGeneration}</td>
                  <td>
                    <span className={`pill ${attempt.status}`}>
                      {attempt.status}
                    </span>
                  </td>
                  <td>{attempt.heartbeats}</td>
                  <td>
                    {session ? (
                      <span className={`pill ${session.status}`}>
                        {session.status}
                      </span>
                    ) : (
                      <span className="muted">—</span>
                    )}
                  </td>
                  <td className="muted">
                    {formatTimestamp(attempt.updatedAt)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}

      <PaseoSessionPanel run={run} attempts={attempts} sessions={sessions} />

      {run.workloadKind !== "paseo-task" && <RunResult run={run} />}

      <h3>Event timeline ({timeline.length})</h3>
      <p className="muted">
        Authoritative lifecycle events from the durable Core ledger, ordered by
        committed <code>globalSeq</code>. Per-tool live activity is not
        recorded; the session evidence above is a bounded final summary.
      </p>
      {timeline.length === 0 ? (
        <p className="muted">No events recorded.</p>
      ) : (
        <ol className="events">
          {timeline.map((event) => {
            const presented = presentEvent(event);
            return (
              <li
                key={event.globalSeq}
                data-event-type={event.eventType}
                data-global-seq={event.globalSeq}
              >
                <div className="event-head">
                  <strong>{presented.label}</strong>{" "}
                  <span className="muted">
                    {event.eventType} · #{event.globalSeq} · seq{" "}
                    {event.sequence}
                  </span>
                </div>
                {presented.detail && (
                  <p className="event-detail">{presented.detail}</p>
                )}
                <span className="muted">
                  <time dateTime={fullTimestamp(event.recordedAt)}>
                    {formatTimestamp(event.recordedAt)}
                  </time>{" "}
                  · {event.actor || "—"}
                </span>
              </li>
            );
          })}
        </ol>
      )}
    </div>
  );
}

function RunResult({ run }: { run: Run }) {
  const result = run.result;
  const mode = stringField(result, "mode");
  const steps = numberField(result, "steps");
  const elapsedMs = numberField(result, "elapsedMs");
  const receipt = stringField(result, "receipt");
  const messageDigest = stringField(result, "messageDigest");
  const imageDigest = stringField(result, "imageDigest");
  const hasResult = result && Object.keys(result).length > 0;

  return (
    <section>
      <h3>Result</h3>
      {!hasResult ? (
        <p className="muted">No result recorded.</p>
      ) : (
        <dl className="detail">
          {mode && (
            <>
              <dt>Mode</dt>
              <dd>
                <code>{mode}</code>
              </dd>
            </>
          )}
          {steps !== null && (
            <>
              <dt>Steps</dt>
              <dd>{steps}</dd>
            </>
          )}
          {elapsedMs !== null && (
            <>
              <dt>Elapsed</dt>
              <dd>{elapsedMs} ms</dd>
            </>
          )}
          {receipt && (
            <>
              <dt>Receipt</dt>
              <dd>
                <code>{receipt}</code>
              </dd>
            </>
          )}
          {messageDigest && (
            <>
              <dt>Message digest</dt>
              <dd>
                <code>{messageDigest}</code>
              </dd>
            </>
          )}
          {imageDigest && (
            <>
              <dt>Image digest</dt>
              <dd>
                <code>{imageDigest}</code>
              </dd>
            </>
          )}
        </dl>
      )}
    </section>
  );
}

function PaseoSessionPanel({
  run,
  attempts,
  sessions,
}: {
  run: Run;
  attempts: Attempt[];
  sessions: PaseoSession[];
}) {
  if (run.workloadKind !== "paseo-task") {
    return (
      <section>
        <h3>Paseo session</h3>
        <p className="muted">
          Not applicable: this Run uses the restricted synthetic fixture and has
          no Paseo session binding.
        </p>
      </section>
    );
  }

  return (
    <section>
      <h3>Paseo session ({sessions.length})</h3>
      {sessions.length === 0 ? (
        <p className="muted">
          No Paseo session binding was recorded for this Run. A queued or leased
          Run may not have reached session intent yet.
        </p>
      ) : (
        sessions.map((session) => {
          const attempt = attempts.find(
            (candidate) => candidate.id === session.attemptId,
          );
          return (
            <SessionDetail
              key={session.attemptId}
              run={run}
              session={session}
              attempt={attempt}
            />
          );
        })
      )}
    </section>
  );
}

function SessionDetail({
  run,
  session,
  attempt,
}: {
  run: Run;
  session: PaseoSession;
  attempt: Attempt | undefined;
}) {
  const result = attempt?.result;
  const observedProvider = stringField(result, "provider");
  const observedModel = stringField(result, "model");
  const observedMode = stringField(result, "paseoMode");
  const hasObserved = Boolean(
    observedProvider && observedModel && observedMode,
  );
  const routeMatches =
    hasObserved &&
    observedProvider === session.provider &&
    observedModel === session.model &&
    observedMode === session.mode;

  return (
    <article className={`session-detail session-${session.status}`}>
      <header className="session-card-head">
        <h4>
          Attempt {session.seq} · generation {session.leaseGeneration}
        </h4>
        <span className={`pill ${session.status}`}>{session.status}</span>
      </header>
      <dl className="detail">
        <dt>Attempt ID</dt>
        <dd>
          <code>{session.attemptId}</code>
        </dd>
        <dt>Worker</dt>
        <dd>
          <Link to="/workers">
            <code>{session.workerId}</code>
          </Link>
        </dd>
        <dt>Execution mode</dt>
        <dd>
          <span className="pill trusted-local">TRUSTED_LOCAL</span>{" "}
          <span className="muted">broad local access, not isolated</span>
        </dd>
        <dt>Requested route</dt>
        <dd>
          <strong>{session.provider}</strong> / {session.model} ·{" "}
          <code>{session.mode}</code>
        </dd>
        <dt>Observed route</dt>
        <dd>
          {hasObserved ? (
            <>
              <strong>{observedProvider}</strong> / {observedModel} ·{" "}
              <code>{observedMode}</code>{" "}
              {routeMatches ? (
                <span className="muted">(matches requested)</span>
              ) : (
                <span className="error">
                  (differs from requested — investigate)
                </span>
              )}
            </>
          ) : (
            <span className="muted">
              Not recorded in final evidence.
              {session.status !== "intended" &&
                " The binding was created only after the Worker asserted the requested route from Paseo evidence."}
            </span>
          )}
        </dd>
        <dt>Session ID</dt>
        <dd>
          {session.sessionId ? (
            <code>{session.sessionId}</code>
          ) : (
            <span className="muted">not bound yet</span>
          )}
        </dd>
        <dt>Workspace ID</dt>
        <dd>
          {session.workspaceId ? (
            <code>{session.workspaceId}</code>
          ) : (
            <span className="muted">not bound yet</span>
          )}
        </dd>
        <dt>Created</dt>
        <dd className="muted">{formatTimestamp(session.createdAt)}</dd>
        <dt>Updated</dt>
        <dd className="muted">{formatTimestamp(session.updatedAt)}</dd>
        {session.error && (
          <>
            <dt>Binding reason</dt>
            <dd className="error">{session.error}</dd>
          </>
        )}
        {run.error && session.status !== "succeeded" && (
          <>
            <dt>Run failure</dt>
            <dd className="error">{run.error}</dd>
          </>
        )}
      </dl>

      {session.status === "intended" && (
        <p className="muted">
          Intent recorded before any external Paseo call. No session identity is
          proven yet.
        </p>
      )}
      {session.status === "active" && (
        <p className="muted">
          Session bound and non-terminal. Its external state may still change.
        </p>
      )}
      {session.status === "cancelled" && (
        <p className="muted">
          The Worker recorded that it never acted on this intent. This is not a
          Core-side stop confirmation.
        </p>
      )}
      {session.status === "orphaned" && (
        <p className="warning">
          External state unconfirmed: the Paseo session may still exist. This is
          not proof that it is running. Core cancellation or a superseded
          Attempt never claims the external session stopped.
          {!session.sessionId &&
            " No session ID was bound, so cleanup must discover it by Run/Attempt/generation labels."}
        </p>
      )}
      {session.status === "failed" && !session.error && (
        <p className="muted">
          The session failed. No external stop confirmation is recorded.
        </p>
      )}

      <SessionEvidence result={result} session={session} />
    </article>
  );
}

function SessionEvidence({
  result,
  session,
}: {
  result: Record<string, unknown> | undefined;
  session: PaseoSession;
}) {
  if (!result || Object.keys(result).length === 0) {
    return (
      <p className="muted">
        No final session summary was recorded for this Attempt. Truthful
        evidence is unavailable, not zero. No cleanup or stop confirmation is
        recorded.
      </p>
    );
  }

  const diffBytes = numberField(result, "diffBytes");
  const diffStat = stringField(result, "diffStat");
  const timelineLines = numberField(result, "timelineLines");
  const toolSteps = numberField(result, "toolSteps");
  const timelineSteps = stringField(result, "timelineSteps");
  const testExit = numberField(result, "testExit");
  const formatExit = numberField(result, "formatExit");
  const nudges = numberField(result, "nudges");
  const cleanup = stringField(result, "cleanup");
  const cleanupOK = cleanup === "ok";

  return (
    <section className="evidence">
      <h4>Session evidence (final summary)</h4>
      <p className="muted">
        Bounded aggregate observed when the session closed. This is not a live
        per-tool feed, and source contents are not stored or displayed.
      </p>
      <dl className="detail">
        {diffBytes !== null && (
          <>
            <dt>Source diff</dt>
            <dd>{diffBytes} bytes</dd>
          </>
        )}
        {diffStat && (
          <>
            <dt>Diff stat</dt>
            <dd>
              <code className="diffstat">{diffStat}</code>
            </dd>
          </>
        )}
        {timelineLines !== null && (
          <>
            <dt>Timeline lines</dt>
            <dd>{timelineLines}</dd>
          </>
        )}
        {toolSteps !== null && (
          <>
            <dt>Tool steps</dt>
            <dd>{toolSteps}</dd>
          </>
        )}
        {timelineSteps && (
          <>
            <dt>Step summary</dt>
            <dd>
              <code>{timelineSteps}</code>
            </dd>
          </>
        )}
        {testExit !== null && (
          <>
            <dt>Tests</dt>
            <dd className={testExit === 0 ? undefined : "error"}>
              {exitLabel(testExit)}
            </dd>
          </>
        )}
        {formatExit !== null && (
          <>
            <dt>Format</dt>
            <dd className={formatExit === 0 ? undefined : "error"}>
              {exitLabel(formatExit)}
            </dd>
          </>
        )}
        {nudges !== null && (
          <>
            <dt>Nudges</dt>
            <dd>{nudges} (bounded continuation)</dd>
          </>
        )}
        <dt>Cleanup / stop</dt>
        <dd className={cleanup && !cleanupOK ? "warning" : "muted"}>
          {cleanup
            ? cleanupOK
              ? "Confirmed by the Worker (stop, archive and workspace archive)"
              : cleanup
            : session.status === "succeeded"
              ? "Not recorded"
              : "Not confirmed in recorded evidence"}
        </dd>
      </dl>
    </section>
  );
}
