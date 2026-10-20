import { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import {
  type Action,
  type ActionSpec,
  type ActionStatus,
  ApiError,
  api,
  type Candidate,
} from "../api";
import { presentEvent } from "../eventPresentation";
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
  const [specs, setSpecs] = useState<ActionSpec[]>([]);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { events: live } = useEventStream(0, (e) => e.aggregateId === id, {
    maxEvents: 200,
  });

  const load = useCallback(async () => {
    if (!id) {
      return;
    }
    try {
      const [nextAction, nextSpecs, nextCandidates] = await Promise.all([
        api.getAction(id),
        api.listActionSpecs(id),
        api.listCandidates({ actionId: id }),
      ]);
      setAction(nextAction);
      setSpecs(nextSpecs);
      setCandidates(nextCandidates);
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
      <section className="candidate-panel" data-testid="action-quality">
        <h3>Frozen specifications ({specs.length})</h3>
        {specs.length === 0 ? (
          <p className="muted">No frozen ActionSpec for this Action.</p>
        ) : (
          specs.map((spec) => (
            <article key={spec.id} className="evidence">
              <header className="session-card-head">
                <code>{spec.id}</code>
                <span className="pill">{spec.riskClass}</span>
              </header>
              <p>{spec.goal}</p>
              <p className="muted">
                content <code>{spec.contentHash}</code> · policy{" "}
                <code>{spec.policyRevision}</code> · profile{" "}
                <code>{spec.qualityProfile}</code>
              </p>
            </article>
          ))
        )}
        <h3>Candidates ({candidates.length})</h3>
        {candidates.length === 0 ? (
          <p className="muted">No CandidateRevision frozen for this Action.</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th>Candidate</th>
                <th>Commit</th>
                <th>State</th>
                <th>Risk</th>
                <th>Implementer Run</th>
              </tr>
            </thead>
            <tbody>
              {candidates.map((candidate) => (
                <tr key={candidate.id}>
                  <td>
                    <code>{candidate.id}</code>
                  </td>
                  <td>
                    <code data-testid="action-candidate-sha">
                      {candidate.commitSha}
                    </code>
                  </td>
                  <td>
                    <span className={`pill candidate-${candidate.state}`}>
                      {candidate.state}
                    </span>
                  </td>
                  <td>{candidate.riskClass}</td>
                  <td>
                    <Link to={`/runs/${candidate.implementerRunId}`}>
                      <code>{candidate.implementerRunId}</code>
                    </Link>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <h3>Event timeline ({timeline.length})</h3>
      {timeline.length === 0 ? (
        <p className="muted">No events recorded.</p>
      ) : (
        <ul className="events">
          {timeline.map((event) => {
            const presented = presentEvent(event);
            return (
              <li key={event.globalSeq}>
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
                  {new Date(event.recordedAt).toLocaleString()} · corr{" "}
                  {event.correlationId ? event.correlationId.slice(0, 8) : "—"}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
