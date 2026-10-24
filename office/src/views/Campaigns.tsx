import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  type Action,
  type Attempt,
  type Campaign,
  type Candidate,
  CoreApiError,
  coreApi,
  type Gate,
  newIdempotencyKey,
  type Run,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

export function Campaigns() {
  const [campaigns, setCampaigns] = useState<Campaign[]>([]);
  const [actions, setActions] = useState<Action[]>([]);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [attempts, setAttempts] = useState<Attempt[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [gates, setGates] = useState<Record<string, Gate | null>>({});
  const [actionId, setActionId] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const attemptKey = useRef(newIdempotencyKey());

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [
          nextCampaigns,
          nextActions,
          nextCandidates,
          nextAttempts,
          nextRuns,
        ] = await Promise.all([
          coreApi.listCampaigns(),
          coreApi.listActions(),
          coreApi.listCandidates(),
          coreApi.listAttempts(),
          coreApi.listRuns(),
        ]);
        const nextGates: Record<string, Gate | null> = {};
        await Promise.all(
          nextCandidates.map(async (candidate) => {
            nextGates[candidate.id] = await coreApi.getGate(candidate.id);
          }),
        );
        if (!active) {
          return;
        }
        setCampaigns(nextCampaigns);
        setActions(nextActions);
        setCandidates(nextCandidates);
        setAttempts(nextAttempts);
        setRuns(nextRuns);
        setGates(nextGates);
        setActionId((current) => current || nextActions[0]?.id || "");
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : "Failed to load Campaigns.",
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
  }, []);

  async function refresh() {
    const [nextCampaigns, nextActions, nextCandidates, nextAttempts, nextRuns] =
      await Promise.all([
        coreApi.listCampaigns(),
        coreApi.listActions(),
        coreApi.listCandidates(),
        coreApi.listAttempts(),
        coreApi.listRuns(),
      ]);
    setCampaigns(nextCampaigns);
    setActions(nextActions);
    setCandidates(nextCandidates);
    setAttempts(nextAttempts);
    setRuns(nextRuns);
    const nextGates: Record<string, Gate | null> = {};
    await Promise.all(
      nextCandidates.map(async (candidate) => {
        nextGates[candidate.id] = await coreApi.getGate(candidate.id);
      }),
    );
    setGates(nextGates);
  }

  async function onCreate(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const campaign = await coreApi.createCampaign({
        action_id: actionId,
        idempotency_key: attemptKey.current,
      });
      attemptKey.current = newIdempotencyKey();
      setNotice(
        `Created Campaign ${shortId(campaign.id)} (${campaign.status}).`,
      );
      await refresh();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Campaign creation failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  async function onAuthorize(campaign: Campaign) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const authorization = await coreApi.authorizeFixtureCampaign(campaign.id);
      setNotice(
        `Authorized Campaign ${shortId(campaign.id)} as ${authorization.risk_class} ${authorization.execution_mode}.`,
      );
      await refresh();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Fixture authorization failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  async function onQueueRun(campaign: Campaign) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const run = await coreApi.createCampaignRun(campaign.id);
      setNotice(
        `Queued Run ${shortId(run.id)}. Open Runs to queue its Attempt.`,
      );
      await refresh();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError ? cause.message : "Run queueing failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  const actionById = new Map(actions.map((action) => [action.id, action]));
  const runByCampaign = new Map<string, Run>();
  for (const run of runs) {
    if (!runByCampaign.has(run.campaign_id)) {
      runByCampaign.set(run.campaign_id, run);
    }
  }
  const attemptById = new Map(attempts.map((attempt) => [attempt.id, attempt]));
  const runById = new Map(runs.map((run) => [run.id, run]));

  function campaignForCandidate(candidate: Candidate): string | undefined {
    const attempt = attemptById.get(candidate.attempt_id);
    return attempt ? runById.get(attempt.run_id)?.campaign_id : undefined;
  }

  return (
    <div>
      <h2>Campaigns</h2>
      <p className="muted">
        A Campaign starts blocked, requires explicit R0 fixture authorization,
        then admits one Run and one Attempt.
      </p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      {actions.length === 0 ? (
        <p className="notice">
          Create an Action before starting a Campaign.{" "}
          <Link to="/actions">Open Actions</Link>
        </p>
      ) : (
        <form className="create" onSubmit={onCreate}>
          <h3>Create a Campaign</h3>
          <label>
            Action
            <select
              data-testid="campaign-action"
              value={actionId}
              onChange={(event) => {
                setActionId(event.target.value);
                attemptKey.current = newIdempotencyKey();
              }}
              required
            >
              {actions.map((action) => (
                <option key={action.id} value={action.id}>
                  {action.objective} ({shortId(action.id)})
                </option>
              ))}
            </select>
          </label>
          <button type="submit" disabled={busy || actionId === ""}>
            {busy ? "Creating…" : "Create Campaign"}
          </button>
        </form>
      )}

      <h3>All Campaigns ({campaigns.length})</h3>
      {campaigns.length === 0 ? (
        <p className="muted">No Campaigns yet.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Campaign</th>
              <th>Action</th>
              <th>Status</th>
              <th>Run</th>
              <th>Created</th>
              <th>Control</th>
            </tr>
          </thead>
          <tbody>
            {campaigns.map((campaign) => {
              const action = actionById.get(campaign.action_id);
              const run = runByCampaign.get(campaign.id);
              return (
                <tr key={campaign.id}>
                  <td>
                    <code>{shortId(campaign.id)}</code>
                  </td>
                  <td>
                    {action ? (
                      action.objective
                    ) : (
                      <code>{shortId(campaign.action_id)}</code>
                    )}
                  </td>
                  <td>
                    <span className="pill">{campaign.status}</span>
                  </td>
                  <td>
                    {run ? (
                      <Link to="/runs">
                        <code>{shortId(run.id)}</code>
                      </Link>
                    ) : (
                      <span className="muted">—</span>
                    )}
                  </td>
                  <td className="muted">
                    {formatTimestamp(campaign.created_at)}
                  </td>
                  <td>
                    {campaign.status === "BLOCKED_AUTHORIZATION" && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void onAuthorize(campaign)}
                      >
                        Authorize R0 fixture
                      </button>
                    )}
                    {campaign.status === "AUTHORIZED" && !run && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void onQueueRun(campaign)}
                      >
                        Queue Run
                      </button>
                    )}
                    {run && <span className="muted">run {run.status}</span>}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}

      <h3>Candidates and quality gates ({candidates.length})</h3>
      <p className="muted">
        Gate decisions are Core-owned and appear only after verifier and
        reviewer receipts exist. No gate means the candidate is still pending.
      </p>
      {candidates.length === 0 ? (
        <p className="muted">No candidates recorded.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Candidate</th>
              <th>Commit</th>
              <th>Attempt</th>
              <th>Implementer</th>
              <th>Campaign</th>
              <th>Gate</th>
              <th>Reasons</th>
              <th>Recorded</th>
            </tr>
          </thead>
          <tbody>
            {candidates.map((candidate) => {
              const gate = gates[candidate.id];
              const campaignId = campaignForCandidate(candidate);
              return (
                <tr key={candidate.id}>
                  <td>
                    <code>{shortId(candidate.id)}</code>
                  </td>
                  <td>
                    <code title={candidate.commit_sha}>
                      {shortId(candidate.commit_sha, 12)}
                    </code>
                  </td>
                  <td>
                    <code>{shortId(candidate.attempt_id)}</code>
                  </td>
                  <td>
                    <code>{candidate.implementer_id}</code>
                  </td>
                  <td>
                    {campaignId ? (
                      <code>{shortId(campaignId)}</code>
                    ) : (
                      <span className="muted">—</span>
                    )}
                  </td>
                  <td>
                    {gate ? (
                      <span
                        className={`pill gate-${gate.outcome.toLowerCase()}`}
                      >
                        {gate.outcome}
                      </span>
                    ) : (
                      <span className="muted">PENDING</span>
                    )}
                  </td>
                  <td className="muted">{gate?.reasons.join("; ") || "—"}</td>
                  <td className="muted">
                    {formatTimestamp(candidate.created_at)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </div>
  );
}
