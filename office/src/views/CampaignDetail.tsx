import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import {
  type CampaignDetail as CampaignDetailData,
  CoreApiError,
  coreApi,
  type Gate,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

export function CampaignDetail() {
  const { campaignId = "" } = useParams();
  const [detail, setDetail] = useState<CampaignDetailData | null>(null);
  const [gates, setGates] = useState<Record<string, Gate | null>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const next = await coreApi.getCampaignDetail(campaignId);
        if (!active) return;
        setDetail(next);
        const entries = await Promise.all(
          next.candidates.map(
            async (candidate) =>
              [candidate.id, await coreApi.getGate(candidate.id)] as const,
          ),
        );
        if (active) setGates(Object.fromEntries(entries));
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : "Failed to load Campaign.",
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => void load(), 4000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [campaignId]);

  if (error) return <p className="error">{error}</p>;
  if (!detail) return <p className="muted">Loading Campaign…</p>;

  const campaign = detail.campaign;
  return (
    <div>
      <h2>Campaign {shortId(campaign.id)}</h2>
      <p className="muted">
        Status: <strong>{campaign.status}</strong> · Action{" "}
        <code>{shortId(campaign.action_id)}</code> · Created{" "}
        {formatTimestamp(campaign.created_at)}
      </p>
      {detail.authorization && (
        <section>
          <h3>Authorization</h3>
          <p>
            {detail.authorization.authorization_id} ·{" "}
            {detail.authorization.risk_class} /{" "}
            {detail.authorization.execution_mode} · actor{" "}
            {detail.authorization.actor}
          </p>
          <p className="muted">{detail.authorization.rationale}</p>
        </section>
      )}

      <h3>WorkGraph ({detail.tasks.length} units)</h3>
      {detail.tasks.length === 0 ? (
        <p className="muted">No Lead WorkGraph tasks.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>#</th>
              <th>Unit</th>
              <th>Status</th>
              <th>Depends on</th>
              <th>Attempt</th>
              <th>Candidate</th>
            </tr>
          </thead>
          <tbody>
            {detail.tasks.map((task) => (
              <tr key={task.task_id}>
                <td>{task.sequence + 1}</td>
                <td>
                  <code>{task.unit_id}</code>
                </td>
                <td>{task.status}</td>
                <td>{task.depends_on.join(", ") || "—"}</td>
                <td>
                  <code>
                    {task.attempt_id ? shortId(task.attempt_id) : "—"}
                  </code>
                </td>
                <td>
                  <code>
                    {task.candidate_commit
                      ? shortId(task.candidate_commit, 12)
                      : "—"}
                  </code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h3>Runs and attempts</h3>
      {detail.runs.length === 0 ? (
        <p className="muted">No Runs.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Run</th>
              <th>Status</th>
              <th>Attempts</th>
            </tr>
          </thead>
          <tbody>
            {detail.runs.map((run) => (
              <tr key={run.id}>
                <td>
                  <code>{shortId(run.id)}</code>
                </td>
                <td>{run.status}</td>
                <td>
                  {detail.attempts
                    .filter((attempt) => attempt.run_id === run.id)
                    .map(
                      (attempt) => `${shortId(attempt.id)}:${attempt.status}`,
                    )
                    .join(", ") || "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h3>Candidates and Quality Gate</h3>
      {detail.candidates.length === 0 ? (
        <p className="muted">No candidates.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Candidate</th>
              <th>Commit</th>
              <th>Implementer</th>
              <th>Gate</th>
            </tr>
          </thead>
          <tbody>
            {detail.candidates.map((candidate) => (
              <tr key={candidate.id}>
                <td>
                  <code>{shortId(candidate.id)}</code>
                </td>
                <td>
                  <code title={candidate.commit_sha}>
                    {shortId(candidate.commit_sha, 12)}
                  </code>
                </td>
                <td>{candidate.implementer_id}</td>
                <td>{gates[candidate.id]?.outcome ?? "NOT_READY"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h3>Events</h3>
      {detail.events.length === 0 ? (
        <p className="muted">No Campaign events.</p>
      ) : (
        <ul className="events">
          {detail.events.slice(-20).map((event) => (
            <li key={event.sequence}>
              <span className="muted">
                {formatTimestamp(event.recorded_at)}
              </span>{" "}
              <strong>{event.kind}</strong> {event.detail}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
