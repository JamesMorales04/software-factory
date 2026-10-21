import { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import {
  ApiError,
  api,
  type CampaignProjection,
  type CandidateBundle,
  type FactoryEvent,
} from "../api";
import { StreamStateBadge } from "../components/StreamStateBadge";
import { presentEvent } from "../eventPresentation";
import { useEventStream } from "../useEventStream";

const ACTIVE = new Set(["pending", "running"]);

export function CampaignDetail() {
  const { id } = useParams<{ id: string }>();
  const [projection, setProjection] = useState<CampaignProjection | null>(null);
  const [bundle, setBundle] = useState<CandidateBundle | null>(null);
  const [ledger, setLedger] = useState<FactoryEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const {
    events: live,
    lastMatchedSeq,
    state: streamState,
  } = useEventStream(
    0,
    (e) => e.aggregateType === "campaign" && e.aggregateId === id,
    { maxEvents: 400 },
  );

  const load = useCallback(async () => {
    if (!id) {
      return;
    }
    try {
      const [next, nextEvents] = await Promise.all([
        api.getCampaign(id),
        api.listCampaignEvents(id),
      ]);
      setProjection(next);
      setLedger(nextEvents);
      if (next.candidate) {
        const nextBundle = await api.getCandidateEvidence(next.candidate.id);
        setBundle(nextBundle);
      } else {
        setBundle(null);
      }
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Failed to load campaign.");
    }
  }, [id]);

  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), 3000);
    return () => clearInterval(timer);
  }, [load]);

  useEffect(() => {
    if (lastMatchedSeq > 0) {
      void load();
    }
  }, [lastMatchedSeq, load]);

  async function start() {
    if (!id) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setProjection(await api.startCampaign(id));
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Start failed.");
    } finally {
      setBusy(false);
    }
  }

  async function cancel() {
    if (!id) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setProjection(await api.cancelCampaign(id));
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Cancel failed.");
    } finally {
      setBusy(false);
    }
  }

  if (error && !projection) {
    return <p className="error">{error}</p>;
  }
  if (!projection) {
    return <p className="muted">Loading…</p>;
  }
  const campaign = projection.campaign;
  const seen = new Set(ledger.map((e) => e.globalSeq));
  const timeline = [
    ...ledger,
    ...live.filter((e) => !seen.has(e.globalSeq)),
  ].sort((a, b) => a.sequence - b.sequence);

  return (
    <div>
      <div className="view-head">
        <h2>
          Campaign <code>{campaign.id.slice(0, 8)}</code>
        </h2>
        <span>
          <span
            className={`pill campaign-${campaign.status}`}
            data-testid="campaign-status"
          >
            {campaign.status}
          </span>{" "}
          <StreamStateBadge state={streamState} />
        </span>
      </div>
      {error && <p className="error">{error}</p>}
      <dl className="detail">
        <dt>Action</dt>
        <dd>
          <Link to={`/actions/${campaign.actionId}`}>
            <code>{campaign.actionId}</code>
          </Link>
        </dd>
        <dt>Frozen spec</dt>
        <dd>
          <code>{campaign.specId}</code> · policy{" "}
          <code>{campaign.policyRevision}</code> · content{" "}
          <code>{campaign.specHash.slice(0, 16)}…</code>
        </dd>
        <dt>Immutable plan</dt>
        <dd>
          <code>{campaign.planHash}</code> · rev {campaign.plan.revision} ·{" "}
          {campaign.plan.tasks.length} tasks
        </dd>
        <dt>Terminal reason</dt>
        <dd data-testid="campaign-reason">
          {campaign.terminalReason || <span className="muted">—</span>}
        </dd>
        <dt>Repair budget</dt>
        <dd data-testid="campaign-repair-budget">
          {projection.budget.repairCycles}/{projection.budget.maxRepairCycles}{" "}
          used
        </dd>
        <dt>Execution budget</dt>
        <dd>
          attempts {projection.budget.attemptsUsed}/
          {projection.budget.maxAttempts} · wall {projection.budget.elapsedSecs}
          s/
          {projection.budget.maxWallClockSecs}s · participants ≤{" "}
          {projection.budget.activeParticipantMax} · provider budget{" "}
          {projection.budget.providerBudget}
        </dd>
      </dl>
      <div className="row">
        {campaign.status === "pending" && (
          <button type="button" onClick={() => void start()} disabled={busy}>
            Start campaign
          </button>
        )}
        {ACTIVE.has(campaign.status) && (
          <button type="button" onClick={() => void cancel()} disabled={busy}>
            Cancel campaign
          </button>
        )}
      </div>

      <section className="candidate-panel">
        <h3>Task graph ({projection.tasks.length})</h3>
        <table>
          <thead>
            <tr>
              <th>Task</th>
              <th>Role</th>
              <th>Cycle</th>
              <th>Status</th>
              <th>Participant / worker</th>
              <th>Candidate / Run</th>
              <th>Outcome</th>
            </tr>
          </thead>
          <tbody>
            {projection.tasks.map((task) => (
              <tr key={task.id} data-testid={`campaign-task-${task.key}`}>
                <td>
                  <code>{task.key}</code>
                </td>
                <td>{task.role}</td>
                <td>{task.cycle}</td>
                <td>
                  <span className={`pill task-${task.status}`}>
                    {task.status}
                  </span>
                </td>
                <td>
                  {task.participantId ? <code>{task.participantId}</code> : "—"}
                  {task.runId && (
                    <>
                      {" "}
                      <Link to={`/runs/${task.runId}`}>
                        <code>{task.runId.slice(0, 8)}</code>
                      </Link>
                    </>
                  )}
                </td>
                <td>
                  {task.candidateId ? (
                    <code>{task.candidateId.slice(0, 8)}</code>
                  ) : (
                    "—"
                  )}
                </td>
                <td className="muted">{task.error || task.outcome || "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="candidate-panel" data-testid="campaign-candidate">
        <h3>Candidate and independent evidence</h3>
        {!projection.candidate ? (
          <p className="muted">No frozen CandidateRevision yet.</p>
        ) : (
          <>
            <p>
              candidate <code>{projection.candidate.id}</code> · commit{" "}
              <code data-testid="campaign-candidate-sha">
                {projection.candidate.commitSha}
              </code>{" "}
              · state {projection.candidate.state} · risk{" "}
              {projection.candidate.riskClass}
            </p>
            {bundle ? (
              <>
                <p data-testid="campaign-evidence">
                  verifier{" "}
                  {bundle.verifications.map((v) => v.actor).join(", ") || "—"} ·{" "}
                  reviewer{" "}
                  {bundle.reviews
                    .map((r) => `${r.actor}:${r.verdict}`)
                    .join(", ") || "—"}
                </p>
                <p>
                  Quality Gate:{" "}
                  <strong data-testid="campaign-gate-status">
                    {projection.decision?.status ?? "pending"}
                  </strong>
                  {projection.decision?.reasons.length
                    ? ` — ${projection.decision.reasons.join("; ")}`
                    : ""}
                </p>
              </>
            ) : (
              <p className="muted">Evidence projection loading…</p>
            )}
          </>
        )}
      </section>

      <h3>Campaign events ({timeline.length})</h3>
      {timeline.length === 0 ? (
        <p className="muted">No campaign events recorded.</p>
      ) : (
        <ul className="events">
          {timeline.map((event) => {
            const presented = presentEvent(event);
            return (
              <li key={event.globalSeq}>
                <div className="event-head">
                  <strong>{presented.label}</strong>{" "}
                  <span className="muted">
                    {event.eventType} · #{event.globalSeq}
                  </span>
                </div>
                {presented.detail && (
                  <p className="event-detail">{presented.detail}</p>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
