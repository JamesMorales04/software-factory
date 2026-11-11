import { ExternalLink, GitMerge, RefreshCw, ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import {
  CoreApiError,
  coreApi,
  type DeliveryIncident,
  type DeliveryStatus,
} from "../../coreApi";
import { formatTimestamp, shortId } from "../../format";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { Checkbox, Input, Textarea } from "../ui/Field";
import { ErrorState, LoadingState, Panel } from "../ui/Surface";
import { StatusBadge } from "./StatusBadge";

function deliveryError(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function activeIncident(status: DeliveryStatus): DeliveryIncident | null {
  return (
    status.projection.incidents.find((incident) =>
      ["OPEN", "ACKNOWLEDGED", "REVERT_PENDING"].includes(incident.state),
    ) ?? null
  );
}

function blockerList(status: DeliveryStatus): string[] {
  const latestAttempt = status.projection.attempts[0] ?? null;
  const decision = status.projection.decisions.at(-1) ?? null;
  const values: string[] = [];
  if (decision && decision.decision === "DENY") {
    values.push(
      ...decision.reasons.map((reason) => `${reason.code}: ${reason.detail}`),
    );
  }
  if (latestAttempt?.blockerCode) {
    values.push(
      `${latestAttempt.blockerCode}: ${
        latestAttempt.blockerDetail ?? latestAttempt.blockerCode
      }`,
    );
  }
  if (status.eligibility?.decision === "DENY") {
    values.push(
      ...status.eligibility.reasons.map(
        (reason) => `${reason.code}: ${reason.detail}`,
      ),
    );
  }
  return [...new Set(values)];
}

export function CampaignDelivery({
  campaignId,
  candidateId,
}: {
  campaignId: string;
  candidateId: string;
}) {
  const { t } = useI18n();
  const [status, setStatus] = useState<DeliveryStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<"open" | "merge" | "revert" | null>(null);
  const [manualConfirmed, setManualConfirmed] = useState(false);
  const [revertPr, setRevertPr] = useState("");
  const [revertSha, setRevertSha] = useState("");
  const [revertRationale, setRevertRationale] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setStatus(await coreApi.getCandidateDelivery(campaignId, candidateId));
    } catch (cause) {
      setError(deliveryError(cause, t("delivery.error")));
    } finally {
      setLoading(false);
    }
  }, [campaignId, candidateId, t]);

  useEffect(() => {
    void load();
  }, [load]);

  async function openPullRequest() {
    setBusy("open");
    setActionError(null);
    setNotice(null);
    try {
      await coreApi.deliverCandidate(campaignId, candidateId, {
        operation: "OPEN_PR",
        manual_confirmed: false,
      });
      setNotice(t("delivery.opened"));
      await load();
    } catch (cause) {
      setActionError(deliveryError(cause, t("delivery.actionError")));
    } finally {
      setBusy(null);
    }
  }

  async function deliver() {
    setBusy("merge");
    setActionError(null);
    setNotice(null);
    try {
      await coreApi.deliverCandidate(campaignId, candidateId, {
        operation: "MERGE",
        manual_confirmed: manualConfirmed,
      });
      setNotice(t("delivery.mergedNotice"));
      await load();
    } catch (cause) {
      setActionError(deliveryError(cause, t("delivery.actionError")));
    } finally {
      setBusy(null);
    }
  }

  async function recordRevert() {
    const incident = status ? activeIncident(status) : null;
    if (!incident) return;
    setBusy("revert");
    setActionError(null);
    setNotice(null);
    try {
      await coreApi.recordDeliveryRevert(incident.incidentId, {
        revert_pr_number: Number(revertPr),
        revert_merge_sha: revertSha.trim(),
        rationale: revertRationale.trim(),
      });
      setNotice(t("delivery.revertRecorded"));
      setRevertPr("");
      setRevertSha("");
      setRevertRationale("");
      await load();
    } catch (cause) {
      setActionError(deliveryError(cause, t("delivery.actionError")));
    } finally {
      setBusy(null);
    }
  }

  if (loading) {
    return (
      <Panel className="delivery-panel" data-testid="delivery-panel">
        <LoadingState label={t("delivery.loading")} />
      </Panel>
    );
  }
  if (error || !status) {
    return (
      <Panel className="delivery-panel" data-testid="delivery-panel">
        <ErrorState
          title={t("delivery.title")}
          message={error ?? t("delivery.error")}
          action={
            <Button variant="secondary" onClick={() => void load()}>
              <RefreshCw size={15} aria-hidden="true" />
              {t("delivery.retry")}
            </Button>
          }
        />
      </Panel>
    );
  }

  const policy = status.policy;
  const latestAttempt = status.projection.attempts[0] ?? null;
  const incident = activeIncident(status);
  const blockers = blockerList(status);
  const gateAccepted = status.gateOutcome === "ACCEPTED";
  const manualControlVisible =
    !policy || policy.mode === "MANUAL" || status.riskClass !== "R0";
  const manualRequired =
    !policy ||
    policy.mode === "MANUAL" ||
    status.riskClass === "R2" ||
    status.riskClass === "R3" ||
    status.riskClass === "UNKNOWN" ||
    (status.riskClass === "R1" && !policy.r1Enabled);
  const merged = latestAttempt?.state === "MERGED";
  const terminalAttempt =
    merged ||
    latestAttempt?.state === "REVERTED" ||
    latestAttempt?.state === "NEEDS_ATTENTION";
  const active = Boolean(incident) || Boolean(policy?.killSwitch);
  const canDeliver =
    gateAccepted && Boolean(policy) && !terminalAttempt && !active;
  const canOpen = Boolean(status.binding) && !terminalAttempt;

  return (
    <Panel className="delivery-panel" data-testid="delivery-panel">
      <div className="delivery-head">
        <div>
          <h3>{t("delivery.title")}</h3>
          <p className="muted">{t("delivery.description")}</p>
        </div>
        <Button variant="quiet" size="sm" onClick={() => void load()}>
          <RefreshCw size={15} aria-hidden="true" />
          {t("delivery.refresh")}
        </Button>
      </div>

      {status.liveError && (
        <p className="warning" role="status">
          {t("delivery.liveUnavailable", { detail: status.liveError })}
        </p>
      )}

      <div className="delivery-grid">
        <section
          className="delivery-card"
          aria-labelledby="delivery-policy-head"
        >
          <h4 id="delivery-policy-head">{t("delivery.policy")}</h4>
          {policy ? (
            <>
              <div className="delivery-status-line">
                <StatusBadge
                  status={
                    policy.mode === "RISK_TIERED" ? "ENABLED" : "DISABLED"
                  }
                  label={
                    policy.mode === "RISK_TIERED"
                      ? t("delivery.policyEnabled")
                      : t("delivery.policyDisabled")
                  }
                />
                {policy.killSwitch && (
                  <StatusBadge status="BLOCKED" label={t("delivery.freeze")} />
                )}
              </div>
              <p className="muted">
                {t("delivery.policyRevision", { revision: policy.revision })} ·{" "}
                {t("delivery.mergeMethod")}: {policy.mergeMethod}
              </p>
              <dl className="delivery-facts">
                <dt>{t("delivery.risk")}</dt>
                <dd>{status.riskClass}</dd>
                <dt>{t("delivery.autoMerge")}</dt>
                <dd>
                  {policy.mode === "RISK_TIERED" && !policy.killSwitch ? (
                    <>
                      <span>{t("delivery.autoMergeR0")}</span>
                      {policy.r1Enabled ? (
                        <span> · {t("delivery.autoMergeR1")}</span>
                      ) : null}
                      <span> · {t("delivery.manualOnly")}</span>
                    </>
                  ) : (
                    t("delivery.policyDisabled")
                  )}
                </dd>
                <dt>{t("delivery.postMergeCheck")}</dt>
                <dd>
                  {policy.postMergeCheckId ||
                    t("delivery.postMergeNotConfigured")}
                </dd>
              </dl>
            </>
          ) : (
            <p className="warning">{t("delivery.policyMissing")}</p>
          )}
        </section>

        <section
          className="delivery-card"
          aria-labelledby="delivery-state-head"
        >
          <h4 id="delivery-state-head">{t("delivery.state")}</h4>
          {latestAttempt ? (
            <>
              <div className="delivery-status-line">
                <StatusBadge status={latestAttempt.state} />
                {latestAttempt.authorization ? (
                  <StatusBadge
                    status="AUTHORIZED"
                    label={latestAttempt.authorization.replaceAll("_", " ")}
                  />
                ) : null}
              </div>
              <dl className="delivery-facts">
                <dt>{t("delivery.candidate")}</dt>
                <dd>
                  <code>{shortId(status.candidateSha, 12)}</code>
                </dd>
                <dt>{t("delivery.gate")}</dt>
                <dd>{status.gateOutcome ?? t("common.notRecorded")}</dd>
                <dt>{t("delivery.base")}</dt>
                <dd>{latestAttempt.baseBranch}</dd>
                <dt>{t("delivery.expectedHead")}</dt>
                <dd>
                  <code>{shortId(latestAttempt.expectedHeadSha, 12)}</code>
                </dd>
                {latestAttempt.mergeCommitSha && (
                  <>
                    <dt>{t("delivery.mergeCommit")}</dt>
                    <dd>
                      <code>{latestAttempt.mergeCommitSha}</code>
                    </dd>
                    <dt>{t("delivery.mergedAt")}</dt>
                    <dd>
                      {latestAttempt.mergedAt
                        ? formatTimestamp(latestAttempt.mergedAt)
                        : t("common.notRecorded")}
                    </dd>
                  </>
                )}
              </dl>
            </>
          ) : (
            <p className="muted">{t("delivery.empty")}</p>
          )}
        </section>

        <section className="delivery-card" aria-labelledby="delivery-pr-head">
          <h4 id="delivery-pr-head">{t("delivery.pullRequest")}</h4>
          {status.pullRequest ? (
            <>
              <a
                href={status.pullRequest.htmlUrl}
                target="_blank"
                rel="noreferrer"
                className="delivery-pr-link"
              >
                <GitMerge size={16} aria-hidden="true" />
                PR #{status.pullRequest.number}
                <ExternalLink size={13} aria-hidden="true" />
              </a>
              <p className="muted">
                {status.pullRequest.state} · {status.pullRequest.headRef} →{" "}
                {status.pullRequest.baseRef}
              </p>
              <p className="muted">
                {t("delivery.base")}: <code>{status.pullRequest.baseSha}</code>
              </p>
            </>
          ) : (
            <p className="muted">{t("delivery.noPullRequest")}</p>
          )}
          <h5>{t("delivery.checks")}</h5>
          {status.checks && status.checks.checkRuns.length > 0 ? (
            <ul className="delivery-checks">
              {status.checks.checkRuns.map((run) => (
                <li key={run.id}>
                  <span>{run.name}</span>
                  <StatusBadge
                    status={run.conclusion.toUpperCase()}
                    label={`${run.status} / ${run.conclusion}`}
                  />
                </li>
              ))}
            </ul>
          ) : (
            <p className="muted">{t("delivery.noChecks")}</p>
          )}
          <h5>{t("delivery.protection")}</h5>
          <p className={status.protection ? "muted" : "warning"}>
            {status.protection
              ? t("delivery.protectionVerified")
              : t("delivery.protectionUnverified")}
          </p>
        </section>

        <section
          className="delivery-card"
          aria-labelledby="delivery-blockers-head"
        >
          <h4 id="delivery-blockers-head">{t("delivery.blockers")}</h4>
          {blockers.length > 0 ? (
            <ul className="delivery-blockers">
              {blockers.map((blocker) => (
                <li key={blocker}>{blocker}</li>
              ))}
            </ul>
          ) : (
            <p className="muted">{t("delivery.noBlockers")}</p>
          )}
          {active && (
            <div className="delivery-freeze" role="status">
              <ShieldAlert size={16} aria-hidden="true" />
              <div>
                <strong>{t("delivery.freeze")}</strong>
                <p>{t("delivery.freezeDetail")}</p>
              </div>
            </div>
          )}
        </section>
      </div>

      {actionError && (
        <p className="ui-error" role="alert">
          {actionError}
        </p>
      )}
      {notice && <p className="notice">{notice}</p>}

      <div className="delivery-actions">
        <Button
          variant="secondary"
          onClick={() => void openPullRequest()}
          busy={busy === "open"}
          disabled={!canOpen || busy !== null}
        >
          {t("delivery.openPullRequest")}
        </Button>
        <Button
          variant="primary"
          onClick={() => void deliver()}
          busy={busy === "merge"}
          disabled={
            !canDeliver || busy !== null || (manualRequired && !manualConfirmed)
          }
        >
          {t("delivery.deliver")}
        </Button>
      </div>
      <p className="muted">{t("delivery.openHint")}</p>
      {manualControlVisible && (
        <div className="delivery-manual">
          <Checkbox
            checked={manualConfirmed}
            onChange={(event) => setManualConfirmed(event.target.checked)}
            label={t("delivery.manualConfirm")}
          />
          <p className="muted">{t("delivery.manualConfirmHint")}</p>
        </div>
      )}

      <section
        className="delivery-card"
        aria-labelledby="delivery-receipts-head"
      >
        <h4 id="delivery-receipts-head">{t("delivery.issueReceipts")}</h4>
        {status.projection.issueReceipts.length === 0 ? (
          <p className="muted">{t("delivery.noIssueReceipts")}</p>
        ) : (
          <ul className="delivery-receipts">
            {status.projection.issueReceipts.map((receipt) => (
              <li key={receipt.receiptId}>
                <StatusBadge status={receipt.state} />
                <span>{receipt.action}</span>
                <code>{receipt.issueLinkId ?? receipt.receiptId}</code>
                {receipt.externalId && (
                  <span className="muted">#{receipt.externalId}</span>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section
        className="delivery-card"
        aria-labelledby="delivery-incident-head"
      >
        <h4 id="delivery-incident-head">{t("delivery.incident")}</h4>
        {incident ? (
          <>
            <div className="delivery-status-line">
              <StatusBadge status={incident.state} />
              <StatusBadge status={incident.kind} />
            </div>
            <p>{incident.detail}</p>
            <p className="muted">
              {t("delivery.mergeCommit")}:{" "}
              <code>{incident.mergeCommitSha}</code>
            </p>
            {incident.state !== "REVERTED" && (
              <fieldset className="delivery-revert">
                <legend className="ui-visually-hidden">
                  {t("delivery.manualRevertRequired")}
                </legend>
                <p className="warning">{t("delivery.manualRevertRequired")}</p>
                <Input
                  label={t("delivery.revertPr")}
                  type="number"
                  min={1}
                  value={revertPr}
                  onChange={(event) => setRevertPr(event.target.value)}
                />
                <Input
                  label={t("delivery.revertSha")}
                  value={revertSha}
                  onChange={(event) => setRevertSha(event.target.value)}
                />
                <Textarea
                  label={t("delivery.revertRationale")}
                  value={revertRationale}
                  onChange={(event) => setRevertRationale(event.target.value)}
                  rows={3}
                />
                <Button
                  variant="danger"
                  busy={busy === "revert"}
                  disabled={
                    busy !== null ||
                    revertPr.trim() === "" ||
                    revertSha.trim() === "" ||
                    revertRationale.trim() === ""
                  }
                  onClick={() => void recordRevert()}
                >
                  {t("delivery.recordRevert")}
                </Button>
              </fieldset>
            )}
          </>
        ) : (
          <p className="muted">{t("delivery.noIncident")}</p>
        )}
        <h5>{t("delivery.reverts")}</h5>
        {status.projection.reverts.length === 0 ? (
          <p className="muted">{t("delivery.noReverts")}</p>
        ) : (
          <ul className="delivery-reverts">
            {status.projection.reverts.map((revert) => (
              <li key={revert.revertId}>
                PR #{revert.revertPrNumber} ·{" "}
                <code>{revert.revertMergeSha}</code> · {revert.actor}
              </li>
            ))}
          </ul>
        )}
      </section>
    </Panel>
  );
}
