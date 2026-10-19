import { useCallback, useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { type Action, type ActionStatus, ApiError, api } from "../api";
import { useEventStream } from "../useEventStream";

const NEXT: Record<ActionStatus, ActionStatus[]> = {
  pending: ["in_progress", "cancelled"],
  in_progress: ["completed", "cancelled"],
  completed: [],
  cancelled: [],
};

export function ActionDetail() {
  const { id } = useParams<{ id: string }>();
  const [action, setAction] = useState<Action | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { events: live } = useEventStream(0, (e) => e.aggregateId === id);

  const load = useCallback(async () => {
    if (!id) {
      return;
    }
    try {
      setAction(await api.getAction(id));
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Failed to load action.");
    }
  }, [id]);

  useEffect(() => {
    void load();
  }, [load]);

  const [ledger, setLedger] = useState<
    Awaited<ReturnType<typeof api.listActionEvents>>
  >([]);
  useEffect(() => {
    if (!id) {
      return;
    }
    api
      .listActionEvents(id)
      .then(setLedger)
      .catch((e: unknown) =>
        setError(e instanceof ApiError ? e.message : "Failed to load events."),
      );
  }, [id]);

  async function transition(to: ActionStatus) {
    if (!id) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const updated = await api.transitionAction(id, to);
      setAction(updated);
      // The transition appended a ledger event; refresh the timeline.
      api
        .listActionEvents(id)
        .then(setLedger)
        .catch(() => {
          // The next poll/stream frame repairs this; do not mask the success.
        });
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Transition failed.");
    } finally {
      setBusy(false);
    }
  }

  if (error && !action) {
    return <p className="error">{error}</p>;
  }
  if (!action) {
    return <p className="muted">Loading…</p>;
  }

  const seen = new Set(ledger.map((e) => e.globalSeq));
  const timeline = [
    ...ledger,
    ...live.filter((e) => !seen.has(e.globalSeq)),
  ].sort((a, b) => a.sequence - b.sequence);

  return (
    <div>
      <h2>{action.title}</h2>
      {error && <p className="error">{error}</p>}
      <dl className="detail">
        <dt>ID</dt>
        <dd>
          <code>{action.id}</code>
        </dd>
        <dt>Project</dt>
        <dd>
          <code>{action.projectId}</code>
        </dd>
        <dt>Status</dt>
        <dd>
          <span className={`pill ${action.status}`}>{action.status}</span>
        </dd>
        <dt>Description</dt>
        <dd>{action.description || <span className="muted">—</span>}</dd>
        <dt>Created</dt>
        <dd className="muted">{new Date(action.createdAt).toLocaleString()}</dd>
        <dt>Updated</dt>
        <dd className="muted">{new Date(action.updatedAt).toLocaleString()}</dd>
      </dl>
      <div className="row">
        {NEXT[action.status].map((to) => (
          <button
            key={to}
            type="button"
            disabled={busy}
            onClick={() => void transition(to)}
          >
            {to === "in_progress"
              ? "Start"
              : to === "completed"
                ? "Complete"
                : "Cancel"}
          </button>
        ))}
        {NEXT[action.status].length === 0 && (
          <span className="muted">Terminal state: no further transitions.</span>
        )}
      </div>
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
