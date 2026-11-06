import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { CancelCampaign } from "../CancelCampaign";
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
import { shortId } from "../format";
import { useI18n } from "../i18n";

export function Campaigns() {
  const { t, formatDate, formatNumber } = useI18n();
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
              : t("campaigns.error.load"),
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
  }, [t]);

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
        t("campaigns.notice.created", {
          id: shortId(campaign.id),
          status: campaign.status,
        }),
      );
      await refresh();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("campaigns.error.create"),
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
        t("campaigns.notice.authorized", {
          id: shortId(campaign.id),
          risk: authorization.riskClass,
          mode: authorization.executionMode,
        }),
      );
      await refresh();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("campaigns.error.authorize"),
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
      setNotice(t("campaigns.notice.queued", { id: shortId(run.id) }));
      await refresh();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("campaigns.error.queueRun"),
      );
    } finally {
      setBusy(false);
    }
  }

  async function onCancelled(campaign: Campaign) {
    setNotice(
      t("campaigns.notice.cancelled", {
        id: shortId(campaign.id),
        status: campaign.status,
      }),
    );
    setError(null);
    try {
      await refresh();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("campaigns.error.refresh"),
      );
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
    <div className="campaigns-workspace">
      <h2>{t("campaigns.title")}</h2>
      <p className="muted">{t("campaigns.description")}</p>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      {actions.length === 0 ? (
        <p className="notice">
          {t("campaigns.needAction")}{" "}
          <Link to="/actions">{t("campaigns.openActions")}</Link>
        </p>
      ) : (
        <form className="create" onSubmit={onCreate}>
          <h3>{t("campaigns.create.title")}</h3>
          <label>
            {t("campaigns.action")}
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
            {busy ? t("campaigns.creating") : t("campaigns.create")}
          </button>
        </form>
      )}

      <h3>{t("campaigns.all", { count: formatNumber(campaigns.length) })}</h3>
      {campaigns.length === 0 ? (
        <p className="muted">{t("campaigns.empty")}</p>
      ) : (
        <div className="table-responsive">
          <table aria-label={t("campaigns.table.label")}>
            <thead>
              <tr>
                <th>{t("campaigns.table.campaign")}</th>
                <th>{t("campaigns.table.action")}</th>
                <th>{t("campaigns.table.status")}</th>
                <th>{t("campaigns.table.run")}</th>
                <th>{t("campaigns.table.created")}</th>
                <th>{t("campaigns.table.control")}</th>
              </tr>
            </thead>
            <tbody>
              {campaigns.map((campaign) => {
                const action = actionById.get(campaign.action_id);
                const run = runByCampaign.get(campaign.id);
                return (
                  <tr key={campaign.id}>
                    <td data-label={t("campaigns.table.campaign")}>
                      <Link to={`/campaigns/${campaign.id}`}>
                        <code>{shortId(campaign.id)}</code>
                      </Link>
                    </td>
                    <td data-label={t("campaigns.table.action")}>
                      {action ? (
                        action.objective
                      ) : (
                        <code>{shortId(campaign.action_id)}</code>
                      )}
                    </td>
                    <td data-label={t("campaigns.table.status")}>
                      <span className="pill">{campaign.status}</span>
                    </td>
                    <td data-label={t("campaigns.table.run")}>
                      {run ? (
                        <Link to="/runs">
                          <code>{shortId(run.id)}</code>
                        </Link>
                      ) : (
                        <span className="muted">—</span>
                      )}
                    </td>
                    <td
                      className="muted"
                      data-label={t("campaigns.table.created")}
                    >
                      {formatDate(campaign.created_at)}
                    </td>
                    <td data-label={t("campaigns.table.control")}>
                      {campaign.status === "BLOCKED_AUTHORIZATION" && (
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void onAuthorize(campaign)}
                        >
                          {t("campaigns.authorize")}
                        </button>
                      )}
                      {campaign.status === "AUTHORIZED" && !run && (
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void onQueueRun(campaign)}
                        >
                          {t("campaigns.queueRun")}
                        </button>
                      )}
                      {run && (
                        <span className="muted">
                          {t("campaigns.runStatus", { status: run.status })}
                        </span>
                      )}
                      <CancelCampaign
                        campaign={campaign}
                        disabled={busy}
                        onCancelled={onCancelled}
                      />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      <h3>
        {t("campaigns.candidates.title", {
          count: formatNumber(candidates.length),
        })}
      </h3>
      <p className="muted">{t("campaigns.candidates.description")}</p>
      {candidates.length === 0 ? (
        <p className="muted">{t("campaigns.candidates.empty")}</p>
      ) : (
        <div className="table-responsive">
          <table
            aria-label={t("campaigns.candidates.title", {
              count: formatNumber(candidates.length),
            })}
          >
            <thead>
              <tr>
                <th>{t("campaigns.table.candidate")}</th>
                <th>{t("campaigns.table.commit")}</th>
                <th>{t("campaigns.table.attempt")}</th>
                <th>{t("campaigns.table.implementer")}</th>
                <th>{t("campaigns.table.campaign")}</th>
                <th>{t("campaigns.table.gate")}</th>
                <th>{t("campaigns.table.reasons")}</th>
                <th>{t("campaigns.table.recorded")}</th>
              </tr>
            </thead>
            <tbody>
              {candidates.map((candidate) => {
                const gate = gates[candidate.id];
                const campaignId = campaignForCandidate(candidate);
                return (
                  <tr key={candidate.id}>
                    <td data-label={t("campaigns.table.candidate")}>
                      <code>{shortId(candidate.id)}</code>
                    </td>
                    <td data-label={t("campaigns.table.commit")}>
                      <code title={candidate.commit_sha}>
                        {shortId(candidate.commit_sha, 12)}
                      </code>
                    </td>
                    <td data-label={t("campaigns.table.attempt")}>
                      <code>{shortId(candidate.attempt_id)}</code>
                    </td>
                    <td data-label={t("campaigns.table.implementer")}>
                      <code>{candidate.implementer_id}</code>
                    </td>
                    <td data-label={t("campaigns.table.campaign")}>
                      {campaignId ? (
                        <Link to={`/campaigns/${campaignId}`}>
                          <code>{shortId(campaignId)}</code>
                        </Link>
                      ) : (
                        <span className="muted">—</span>
                      )}
                    </td>
                    <td data-label={t("campaigns.table.gate")}>
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
                    <td
                      className="muted"
                      data-label={t("campaigns.table.reasons")}
                    >
                      {gate?.reasons.join("; ") || "—"}
                    </td>
                    <td
                      className="muted"
                      data-label={t("campaigns.table.recorded")}
                    >
                      {formatDate(candidate.created_at)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
