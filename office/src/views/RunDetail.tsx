import { useCallback, useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import {
  ApiError,
  type Attempt,
  api,
  type FactoryEvent,
  type Run,
} from "../api";
import { useEventStream } from "../useEventStream";

export function RunDetail() {
  const { id } = useParams<{ id: string }>();
  const [run, setRun] = useState<Run | null>(null);
  const [attempts, setAttempts] = useState<Attempt[]>([]);
  const [ledger, setLedger] = useState<FactoryEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const { events: live } = useEventStream(0, (e) => e.aggregateId === id);

  const load = useCallback(async () => {
    if (!id) {
      return;
    }
    try {
      const [r, a, ev] = await Promise.all([
        api.getRun(id),
        api.listAttempts(id),
        api.listRunEvents(id),
      ]);
      setRun(r);
      setAttempts(a);
      setLedger(ev);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Failed to load run.");
    }
  }, [id]);

  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), 2000);
    return () => clearInterval(timer);
  }, [load]);

  if (error && !run) {
    return <p className="error">{error}</p>;
  }
  if (!run) {
    return <p className="muted">Loading…</p>;
  }

  const seen = new Set(ledger.map((e) => e.globalSeq));
  const timeline = [
    ...ledger,
    ...live.filter((e) => !seen.has(e.globalSeq)),
  ].sort((a, b) => a.sequence - b.sequence);

  return (
    <div>
      <h2>
        Run <code>{run.id.slice(0, 8)}</code>
      </h2>
      {error && <p className="error">{error}</p>}
      <dl className="detail">
        <dt>ID</dt>
        <dd>
          <code>{run.id}</code>
        </dd>
        <dt>Status</dt>
        <dd>
          <span className={`pill ${run.status}`}>{run.status}</span>
        </dd>
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
        <dt>Result</dt>
        <dd>
          {run.result ? (
            <pre>{JSON.stringify(run.result, null, 2)}</pre>
          ) : (
            <span className="muted">—</span>
          )}
        </dd>
        {run.error && (
          <>
            <dt>Error</dt>
            <dd className="error">{run.error}</dd>
          </>
        )}
        <dt>Created</dt>
        <dd className="muted">{new Date(run.createdAt).toLocaleString()}</dd>
        <dt>Updated</dt>
        <dd className="muted">{new Date(run.updatedAt).toLocaleString()}</dd>
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
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            {attempts.map((a) => (
              <tr key={a.id}>
                <td>{a.seq}</td>
                <td>
                  <code>{a.workerId}</code>
                </td>
                <td>{a.leaseGeneration}</td>
                <td>
                  <span className={`pill ${a.status}`}>{a.status}</span>
                </td>
                <td>{a.heartbeats}</td>
                <td className="muted">
                  {new Date(a.updatedAt).toLocaleString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <h3>Event timeline ({timeline.length})</h3>
      {timeline.length === 0 ? (
        <p className="muted">No events recorded.</p>
      ) : (
        <ul className="events">
          {timeline.map((e) => (
            <li key={e.globalSeq}>
              <code>seq {e.sequence}</code> <strong>{e.eventType}</strong>{" "}
              <span className="muted">
                {new Date(e.recordedAt).toLocaleString()} · corr{" "}
                {e.correlationId ? e.correlationId.slice(0, 8) : "—"}
              </span>
              <pre>{JSON.stringify(e.payload, null, 2)}</pre>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
