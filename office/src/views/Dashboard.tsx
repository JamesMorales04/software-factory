import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ApiError, api } from "../api";
import { StreamStateBadge } from "../components/StreamStateBadge";
import { useEventStream } from "../useEventStream";

export function Dashboard() {
  const [health, setHealth] = useState<{ version: string } | null>(null);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [runCounts, setRunCounts] = useState<Record<string, number>>({});
  const [workerCounts, setWorkerCounts] = useState<Record<string, number>>({});
  const [error, setError] = useState<string | null>(null);
  const { events, state } = useEventStream(0);

  useEffect(() => {
    let live = true;
    (async () => {
      try {
        const [h, actions, runs, workers] = await Promise.all([
          api.health(),
          api.listActions(),
          api.listRuns().catch(() => []),
          api.listWorkers().catch(() => []),
        ]);
        if (!live) {
          return;
        }
        setHealth({ version: h.version });
        const byStatus: Record<string, number> = { total: actions.length };
        for (const a of actions) {
          byStatus[a.status] = (byStatus[a.status] ?? 0) + 1;
        }
        setCounts(byStatus);
        const byRun: Record<string, number> = { total: runs.length };
        for (const r of runs) {
          byRun[r.status] = (byRun[r.status] ?? 0) + 1;
        }
        setRunCounts(byRun);
        const byWorker: Record<string, number> = { total: workers.length };
        for (const w of workers) {
          byWorker[w.status] = (byWorker[w.status] ?? 0) + 1;
        }
        setWorkerCounts(byWorker);
      } catch (e) {
        if (live) {
          setError(
            e instanceof ApiError ? e.message : "Failed to load dashboard.",
          );
        }
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  if (error) {
    return <p className="error">{error}</p>;
  }
  return (
    <div>
      <h2>Dashboard</h2>
      <section className="cards">
        <div className="card">
          <h3>Core</h3>
          <p>{health ? `version ${health.version}` : "loading…"}</p>
        </div>
        <div className="card">
          <h3>Actions</h3>
          <p>
            {counts.total ?? "…"} total · {counts.pending ?? 0} pending ·{" "}
            {counts.in_progress ?? 0} in progress · {counts.completed ?? 0}{" "}
            completed · {counts.cancelled ?? 0} cancelled
          </p>
          <Link to="/actions">Open Actions</Link>
        </div>
        <div className="card">
          <h3>Runs</h3>
          <p>
            {runCounts.total ?? "…"} total · {runCounts.queued ?? 0} queued ·{" "}
            {runCounts.leased ?? 0} leased · {runCounts.running ?? 0} running ·{" "}
            {runCounts.succeeded ?? 0} succeeded · {runCounts.failed ?? 0}{" "}
            failed · {runCounts.cancelled ?? 0} cancelled
          </p>
          <Link to="/runs">Open Runs</Link>
        </div>
        <div className="card">
          <h3>Workers</h3>
          <p>
            {workerCounts.total ?? "…"} registered · {workerCounts.online ?? 0}{" "}
            online · {workerCounts.offline ?? 0} offline
          </p>
          <Link to="/workers">Open Workers</Link>
        </div>
        <div className="card">
          <h3>Event stream</h3>
          <p>
            state: <StreamStateBadge state={state} /> · {events.length} events
            observed this session
          </p>
        </div>
      </section>
      <h3>Recent events (live)</h3>
      {events.length === 0 ? (
        <p className="muted">
          No events observed yet. Create an Action to see the ledger move.
        </p>
      ) : (
        <ul className="events">
          {[...events]
            .reverse()
            .slice(0, 20)
            .map((e) => (
              <li key={e.globalSeq}>
                <code>#{e.globalSeq}</code> <strong>{e.eventType}</strong>{" "}
                <span className="muted">
                  {e.aggregateType}/{e.aggregateId.slice(0, 8)} seq {e.sequence}
                </span>
              </li>
            ))}
        </ul>
      )}
    </div>
  );
}
