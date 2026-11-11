import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { LinkedIssues } from "../components/office/LinkedIssues";
import { StatusBadge } from "../components/office/StatusBadge";
import { Badge } from "../components/ui/Badge";
import { Button, buttonClass } from "../components/ui/Button";
import { Checkbox, Textarea } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  CoreApiError,
  coreApi,
  type IntakeRequestProjection,
  newIdempotencyKey,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";
import { EnvironmentPanel } from "./EnvironmentPanel";
import { RequestAssessment } from "./RequestAssessment";
import { RequestPlan } from "./RequestPlan";
import {
  IntakeModeBadge,
  IntakeStateBadge,
  isTerminalIntakeState,
  supportsIntakeFollowUp,
} from "./RequestState";
import { RequestTrace } from "./RequestTrace";

function errorMessage(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError
    ? cause.message
    : cause instanceof Error
      ? cause.message
      : fallback;
}

function isNotFound(cause: unknown): boolean {
  return cause instanceof CoreApiError && cause.status === 404;
}

function messageRoleLabel(
  role: string,
  t: ReturnType<typeof useI18n>["t"],
): string {
  if (role === "operator") return t("request.conversation.roleOperator");
  if (role === "system") return t("request.conversation.roleSystem");
  return role.replaceAll("_", " ");
}

export function RequestWorkspace() {
  const { requestId = "" } = useParams<{ requestId: string }>();
  const { t, formatDate, formatNumber } = useI18n();
  const navigate = useNavigate();
  const [data, setData] = useState<IntakeRequestProjection | null>(null);
  const [projectName, setProjectName] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notFound, setNotFound] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [fencedCampaignId, setFencedCampaignId] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [rationale, setRationale] = useState("");
  const [approvedScope, setApprovedScope] = useState(false);
  const [extraUsageConfirmed, setExtraUsageConfirmed] = useState(false);
  const [messageBody, setMessageBody] = useState("");
  const [cancelOpen, setCancelOpen] = useState(false);
  const [repairOpen, setRepairOpen] = useState(false);
  const [repairReason, setRepairReason] = useState("");
  const [repairConfirmed, setRepairConfirmed] = useState(false);
  const [repairBusy, setRepairBusy] = useState(false);
  const [repairError, setRepairError] = useState<string | null>(null);
  const [cancelReason, setCancelReason] = useState("");
  const messageKey = useRef(newIdempotencyKey());

  const load = useCallback(
    async (background = false) => {
      try {
        const next = await coreApi.getIntakeRequest(requestId);
        setData(next);
        void coreApi
          .listProjects()
          .then((projects) => {
            const project = projects.find((item) => item.id === next.projectId);
            if (project) setProjectName(project.name || project.repository);
          })
          .catch(() => undefined);
        setLoadError(null);
        setNotFound(false);
        setConnectionError(null);
        setLastLoadedAt(new Date().toISOString());
      } catch (cause) {
        const message = errorMessage(cause, t("request.load.error"));
        if (background) {
          setConnectionError(message);
        } else {
          setLoadError(message);
          setNotFound(isNotFound(cause));
        }
      }
    },
    [requestId, t],
  );

  useEffect(() => {
    setData(null);
    setLoadError(null);
    setNotFound(false);
    setConnectionError(null);
    setLastLoadedAt(null);
    setActionError(null);
    setNotice(null);
    setFencedCampaignId(null);
    setRationale("");
    setApprovedScope(false);
    setMessageBody("");
    setCancelOpen(false);
    setCancelReason("");
    messageKey.current = newIdempotencyKey();
    void load(false);
    const timer = setInterval(() => void load(true), 5000);
    return () => clearInterval(timer);
  }, [load]);

  async function runAction(
    kind: string,
    operation: () => Promise<void>,
    success: string,
  ): Promise<boolean> {
    setBusy(kind);
    setActionError(null);
    setNotice(null);
    try {
      await operation();
      await load(true);
      setNotice(success);
      return true;
    } catch (cause) {
      if (
        cause instanceof CoreApiError &&
        cause.code === "LEAD_ALREADY_STARTED"
      ) {
        const body =
          typeof cause.body === "object" && cause.body !== null
            ? (cause.body as {
                campaign_id?: unknown;
                campaign_status?: unknown;
              })
            : null;
        const campaignId =
          typeof body?.campaign_id === "string" ? body.campaign_id : null;
        setFencedCampaignId(campaignId);
        setActionError(
          campaignId
            ? t("request.fenced.withId", { id: shortId(campaignId) })
            : t("request.fenced.withoutId"),
        );
        return false;
      }
      setActionError(errorMessage(cause, t("request.action.error")));
      return false;
    } finally {
      setBusy(null);
    }
  }

  if (data === null) {
    if (loadError !== null) {
      return (
        <ErrorState
          title={notFound ? t("request.notFound") : t("request.load.title")}
          message={loadError}
          action={
            <div className="request-error-actions">
              <Button onClick={() => void load(false)}>
                {t("common.retry")}
              </Button>
              <Link className={buttonClass("quiet")} to="/">
                {t("request.backHome")}
              </Link>
            </div>
          }
        />
      );
    }
    return <LoadingState label={t("request.loading")} />;
  }

  const terminal = isTerminalIntakeState(data.state);
  const followUpEligible =
    supportsIntakeFollowUp(data.state) &&
    data.authorization.reason !== "INTAKE_WITHDRAWN";
  const followUpRequest = data.followUpRequest ?? null;
  const canFollowUp = followUpEligible && followUpRequest === null;
  const canMessage = (!terminal && data.plan === null) || canFollowUp;
  const canCancel = !terminal && data.state !== "EXECUTING";
  const authorizationPending = [
    "AWAITING_APPROVAL",
    "BLOCKED",
    "FAILED",
  ].includes(data.state);
  const authorizationAvailable = data.authorization.available;
  const isAutopilot = data.mode === "AUTONOMOUS";
  const offline = connectionError !== null;
  const planning = data.planning ?? null;
  const planningReady = planning?.ready === true;
  const missingPlanArtifacts = planning?.missingArtifacts ?? [];
  const participantPlan = planning?.participantPlan ?? null;
  const participantPlanIncomplete =
    participantPlan?.required === true && participantPlan.ready !== true;
  const planningIncomplete =
    data.state === "AWAITING_APPROVAL" && data.plan !== null && !planningReady;
  const planningProblemValues = participantPlan
    ? [
        ...participantPlan.missingRoles,
        ...participantPlan.unexpectedRoles,
        ...participantPlan.missingAssignments,
      ]
    : [];
  const approvalReady =
    data.state === "AWAITING_APPROVAL" && data.plan !== null;
  const approveDisabled =
    busy !== null ||
    offline ||
    !approvalReady ||
    !planningReady ||
    !authorizationAvailable ||
    rationale.trim() === "" ||
    !approvedScope;

  async function assess() {
    if (!data) return;
    await runAction(
      "assess",
      async () => {
        await coreApi.assessIntakeRequest(data.id);
      },
      t("request.notice.assessed"),
    );
  }

  async function confirmSubscription() {
    if (!data || !extraUsageConfirmed) return;
    await runAction(
      "subscription",
      async () => {
        const now = Date.now();
        await coreApi.createIntakeSubscriptionAdmission(data.id, {
          assumption_id: `owner-${data.id}-${now}`,
          confirmed_by: "Product Owner",
          confirmed_at_ms: now,
          valid_until_ms: now + 24 * 60 * 60 * 1000,
          extra_usage_disabled: true,
          statement:
            "I confirm OpenCode Go Extra Usage is disabled for this Factory subscription route.",
          test_profile_id: "opencode-go-subscription",
          idempotency_key: newIdempotencyKey(),
        });
      },
      t("request.subscription.confirmed"),
    );
  }

  async function completeAssessment() {
    if (!data) return;
    await runAction(
      "freeze",
      async () => {
        await coreApi.assessIntakeRequest(data.id);
      },
      t("request.notice.frozen"),
    );
  }

  async function approve(event: React.FormEvent) {
    event.preventDefault();
    if (!data?.plan) return;
    if (!authorizationAvailable || rationale.trim() === "" || !approvedScope) {
      return;
    }
    const planHash = data.plan.plan_hash;
    const ok = await runAction(
      "approve",
      async () => {
        await coreApi.approveIntakeRequest(data.id, {
          expected_plan_hash: planHash,
          rationale: rationale.trim(),
        });
      },
      t("request.notice.approved"),
    );
    if (ok) {
      setRationale("");
      setApprovedScope(false);
    }
  }

  async function sendMessage(event: React.FormEvent) {
    event.preventDefault();
    if (!data) return;
    const body = messageBody.trim();
    if (body === "") return;
    const ok = await runAction(
      "message",
      async () => {
        await coreApi.messageIntakeRequest(data.id, {
          body,
          idempotency_key: messageKey.current,
        });
      },
      canFollowUp
        ? t("request.notice.followUpStarted")
        : t("request.notice.message"),
    );
    if (ok) {
      messageKey.current = newIdempotencyKey();
      setMessageBody("");
    }
  }

  async function repairFromCandidate(event: React.FormEvent) {
    event.preventDefault();
    if (!data?.campaignId || !repairConfirmed) return;
    setRepairBusy(true);
    setRepairError(null);
    try {
      const result = await coreApi.repairCampaign(data.campaignId, {
        confirm: true,
        reason: repairReason.trim(),
      });
      navigate(`/requests/${encodeURIComponent(result.requestId)}`);
    } catch (cause) {
      setRepairError(errorMessage(cause, t("request.repair.error")));
    } finally {
      setRepairBusy(false);
    }
  }

  async function cancelRequest(event: React.FormEvent) {
    event.preventDefault();
    if (!data || cancelReason.trim() === "") return;
    const ok = await runAction(
      "cancel",
      async () => {
        await coreApi.cancelIntakeRequest(data.id, cancelReason.trim());
      },
      t("request.notice.cancelled"),
    );
    if (ok) {
      setCancelOpen(false);
      setCancelReason("");
    }
  }

  return (
    <div className="request-workspace">
      <PageHeader
        title={data.objective}
        description={t("request.workspace.description")}
        meta={
          <>
            <span>
              {t("request.meta.request")} <code>{shortId(data.id)}</code>
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              {t("request.meta.project")}{" "}
              {projectName ?? shortId(data.projectId)}
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              {t("request.meta.profile")}{" "}
              {t("request.meta.profileValue", {
                name: data.profile.name,
                revision: data.profile.revision,
              })}
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              {t("request.meta.updated", {
                date: formatDate(data.updatedAt),
              })}
            </span>
          </>
        }
        actions={
          <>
            <IntakeStateBadge state={data.state} />
            <IntakeModeBadge mode={data.mode} />
            {data.campaignId && (
              <Link
                className={buttonClass("secondary")}
                to={`/campaigns/${encodeURIComponent(data.campaignId)}`}
              >
                {t("request.openCampaign")}
              </Link>
            )}
            <Link className={buttonClass("quiet")} to="/">
              {t("request.backHome")}
            </Link>
          </>
        }
      />

      {connectionError && (
        <div
          className="ui-callout ui-callout--attention request-callout"
          role="status"
        >
          <p>
            {t("request.offline", {
              error: connectionError,
              loadedAt: formatDate(lastLoadedAt ?? data.updatedAt),
              updatedAt: formatDate(data.updatedAt),
            })}
          </p>
          <Button size="sm" onClick={() => void load(true)}>
            {t("request.retryNow")}
          </Button>
        </div>
      )}

      {data.stateDetail && (
        <p
          className={`ui-callout ${
            data.state === "BLOCKED" || data.state === "FAILED"
              ? "ui-callout--danger"
              : "ui-callout--info"
          } request-callout`}
          role="status"
        >
          {data.stateDetail}
        </p>
      )}

      {authorizationPending &&
        data.authorization.required &&
        data.authorization.available && (
          <p className="ui-callout ui-callout--attention request-callout">
            <strong>{t("request.authorization.requiredTitle")}</strong>{" "}
            {t("request.authorization.requiredBody")}
          </p>
        )}
      {authorizationPending &&
        data.authorization.required &&
        !data.authorization.available && (
          <p
            className="ui-callout ui-callout--danger request-callout"
            role="alert"
          >
            <strong>{t("request.authorization.unavailableTitle")}</strong>{" "}
            {data.authorization.reason ??
              t("request.authorization.unavailableFallback")}{" "}
            {t("request.authorization.unavailableBody")}
          </p>
        )}
      {authorizationPending && !data.authorization.required && (
        <p className="ui-callout ui-callout--info request-callout">
          {t("request.authorization.notRequired")}
        </p>
      )}

      {planningIncomplete && (
        <div
          className="ui-callout ui-callout--attention request-callout"
          role="status"
        >
          <p>
            <strong>{t("request.planning.incompleteTitle")}</strong>{" "}
            {t("request.planning.incompleteBody")}
          </p>
          {planning === null ? (
            <p>{t("request.planning.unavailable")}</p>
          ) : (
            <>
              {missingPlanArtifacts.length > 0 && (
                <p>
                  {t("request.planning.missingArtifacts", {
                    values: missingPlanArtifacts.join(", "),
                  })}
                </p>
              )}
              {participantPlanIncomplete && participantPlan && (
                <p>
                  {participantPlan.present
                    ? t("request.planning.participantIncomplete", {
                        values:
                          planningProblemValues.length > 0
                            ? planningProblemValues.join(", ")
                            : t("request.planning.participantAssignments"),
                      })
                    : t("request.planning.participantMissing")}
                </p>
              )}
              {!planning.reviewApproved && (
                <p>
                  {planning.reviewStatus === "PENDING"
                    ? t("request.planning.reviewPending")
                    : t("request.planning.reviewNotApproved", {
                        status: planning.reviewStatus ?? "—",
                      })}
                </p>
              )}
              {participantPlan?.present &&
                planningProblemValues.length === 0 &&
                participantPlan.problems.length > 0 && (
                  <ul className="muted">
                    {participantPlan.problems.map((problem) => (
                      <li key={problem}>{problem}</li>
                    ))}
                  </ul>
                )}
            </>
          )}
          <Link
            className={buttonClass("secondary")}
            to={`/director?request=${encodeURIComponent(data.id)}`}
          >
            {t("request.planning.openDirector")}
          </Link>
        </div>
      )}

      {actionError && (
        <div className="ui-callout ui-callout--danger" role="alert">
          <p>{actionError}</p>
          {fencedCampaignId && (
            <Link
              className={buttonClass("secondary")}
              to={`/campaigns/${encodeURIComponent(fencedCampaignId)}`}
            >
              {t("request.openCampaign")}
            </Link>
          )}
        </div>
      )}
      {notice && (
        <p className="notice" role="status" aria-live="polite">
          {notice}
        </p>
      )}

      <LinkedIssues
        links={data.linked_issues}
        onRetried={() => void load(true)}
      />

      <div className="request-layout">
        <div className="request-column">
          {data.state === "ASSESSED" && (
            <Panel className="request-panel">
              <p className="ui-callout ui-callout--info" role="status">
                {t("request.assessed.callout")}
              </p>
            </Panel>
          )}

          {(data.state === "SUBMITTED" ||
            data.state === "ASSESSING" ||
            data.state === "PLAN_READY" ||
            data.state === "AWAITING_APPROVAL") && (
            <Panel className="request-panel request-next-step">
              <div className="section-heading">
                <h2>{t("request.next.title")}</h2>
                <span className="muted">{t("request.next.hint")}</span>
              </div>
              {isAutopilot ? (
                <p className="ui-callout ui-callout--info" role="status">
                  {t("request.autopilot.status")}
                </p>
              ) : (
                <>
                  {data.state === "SUBMITTED" && (
                    <>
                      <p className="muted">
                        {t("request.next.assessDescription")}
                      </p>
                      <Button
                        variant="primary"
                        disabled={busy !== null || offline}
                        onClick={() => void assess()}
                      >
                        {busy === "assess"
                          ? t("request.next.assessing")
                          : t("request.next.assess")}
                      </Button>
                      <details className="request-subscription">
                        <summary>{t("request.subscription.title")}</summary>
                        <p className="muted">
                          {t("request.subscription.help")}
                        </p>
                        <Checkbox
                          label={t("request.subscription.confirm")}
                          checked={extraUsageConfirmed}
                          onChange={(event) =>
                            setExtraUsageConfirmed(event.target.checked)
                          }
                          disabled={busy !== null || offline}
                        />
                        <Button
                          variant="secondary"
                          disabled={
                            busy !== null || offline || !extraUsageConfirmed
                          }
                          onClick={() => void confirmSubscription()}
                        >
                          {busy === "subscription"
                            ? t("request.subscription.saving")
                            : t("request.subscription.save")}
                        </Button>
                      </details>
                    </>
                  )}
                  {data.state === "ASSESSING" && (
                    <>
                      <p className="muted" role="status">
                        {t("request.next.assessingDescription")}
                      </p>
                      <Button
                        disabled={busy !== null}
                        onClick={() => void load(true)}
                      >
                        {t("request.next.refresh")}
                      </Button>
                    </>
                  )}
                  {data.state === "PLAN_READY" &&
                    (data.proposal ? (
                      <>
                        <p className="muted">
                          {t("request.next.freezeDescription")}
                        </p>
                        <Button
                          variant="primary"
                          disabled={busy !== null || offline}
                          onClick={() => void completeAssessment()}
                        >
                          {busy === "freeze"
                            ? t("request.next.freezing")
                            : t("request.next.freeze")}
                        </Button>
                      </>
                    ) : (
                      <p className="muted">
                        {t("request.next.readyNoProposal")}
                      </p>
                    ))}
                  {data.state === "AWAITING_APPROVAL" &&
                    (data.plan === null ? (
                      <p className="ui-callout ui-callout--danger">
                        {t("request.next.awaitingNoPlan")}
                      </p>
                    ) : (
                      <form
                        className="request-approval"
                        onSubmit={(event) => void approve(event)}
                      >
                        <p className="muted">
                          {t("request.approval.description")}
                        </p>
                        <dl className="request-facts">
                          <div>
                            <dt>{t("request.approval.planHash")}</dt>
                            <dd>
                              <code title={data.plan.plan_hash}>
                                {data.plan.plan_hash}
                              </code>
                            </dd>
                          </div>
                          <div>
                            <dt>{t("request.approval.profileRevision")}</dt>
                            <dd>
                              {data.profile.name} r{data.profile.revision} ·{" "}
                              <code title={data.profile.contentHash}>
                                {shortId(data.profile.contentHash, 12)}
                              </code>
                            </dd>
                          </div>
                        </dl>
                        {!authorizationAvailable && (
                          <p className="ui-callout ui-callout--danger">
                            {data.authorization.reason ??
                              t("request.approval.reasonFallback")}
                          </p>
                        )}
                        <Textarea
                          label={t("request.approval.rationale")}
                          value={rationale}
                          onChange={(event) => setRationale(event.target.value)}
                          disabled={
                            !authorizationAvailable || offline || !planningReady
                          }
                          help={t("request.approval.rationaleHelp")}
                          required
                        />
                        <Checkbox
                          label={t("request.approval.reviewed")}
                          checked={approvedScope}
                          onChange={(event) =>
                            setApprovedScope(event.target.checked)
                          }
                          disabled={
                            !authorizationAvailable || offline || !planningReady
                          }
                        />
                        <div className="request-form-actions">
                          <Button
                            type="submit"
                            variant="primary"
                            disabled={approveDisabled}
                          >
                            {busy === "approve"
                              ? t("request.approval.record")
                              : t("request.approval.approve")}
                          </Button>
                          {!authorizationAvailable && (
                            <span className="muted">
                              {t("request.approval.disabled")}
                            </span>
                          )}
                          {authorizationAvailable && !planningReady && (
                            <span className="muted">
                              {t("request.approval.planningDisabled")}
                            </span>
                          )}
                        </div>
                      </form>
                    ))}
                </>
              )}
            </Panel>
          )}

          {data.state === "EXECUTING" && (
            <Panel className="request-panel">
              <h2>{t("request.execution.title")}</h2>
              <p className="muted">{t("request.execution.description")}</p>
              {data.campaignId ? (
                <Link
                  className={buttonClass("primary")}
                  to={`/campaigns/${encodeURIComponent(data.campaignId)}`}
                >
                  {t("request.openCampaignWithId", {
                    id: shortId(data.campaignId),
                  })}
                </Link>
              ) : (
                <p className="ui-callout ui-callout--attention">
                  {t("request.execution.missing")}
                </p>
              )}
            </Panel>
          )}

          {data.state === "BLOCKED" && (
            <Panel className="request-panel">
              <h2>{t("request.blocked.title")}</h2>
              <p className="muted">{t("request.blocked.description")}</p>
              <Link className={buttonClass("secondary")} to="/">
                {t("request.newRequest")}
              </Link>
            </Panel>
          )}

          {data.state === "FAILED" && (
            <Panel className="request-panel">
              <h2>{t("request.failed.title")}</h2>
              <p className="muted">{t("request.failed.description")}</p>
              <Link className={buttonClass("secondary")} to="/">
                {t("request.newRequest")}
              </Link>
            </Panel>
          )}

          {data.state === "COMPLETED" && (
            <Panel className="request-panel">
              <h2>{t("request.completed.title")}</h2>
              <p className="muted">{t("request.completed.description")}</p>
              {data.campaignId && (
                <Link
                  className={buttonClass("secondary")}
                  to={`/campaigns/${encodeURIComponent(data.campaignId)}`}
                >
                  {t("request.openCampaign")}
                </Link>
              )}
            </Panel>
          )}

          {data.state === "CANCELLED" && (
            <Panel className="request-panel">
              <h2>{t("request.cancelled.title")}</h2>
              <p className="muted">{t("request.cancelled.description")}</p>
            </Panel>
          )}

          <RequestAssessment assessment={data.assessment} />
          <RequestPlan proposal={data.proposal} plan={data.plan} />
          <EnvironmentPanel
            environment={data.environment ?? null}
            requestId={data.id}
          />
          <RequestTrace requestId={data.id} />

          {(data.campaignId || data.campaign) && (
            <Panel className="request-panel">
              <div className="section-heading">
                <h2>{t("request.campaign.title")}</h2>
                {data.gateOutcome && (
                  <Badge
                    tone={data.gateOutcome === "ACCEPTED" ? "success" : "muted"}
                  >
                    {t("request.campaign.gate", {
                      outcome: data.gateOutcome,
                    })}
                  </Badge>
                )}
              </div>
              <dl className="request-facts">
                <div>
                  <dt>{t("request.campaign.campaign")}</dt>
                  <dd>
                    {data.campaignId ? (
                      <Link
                        to={`/campaigns/${encodeURIComponent(data.campaignId)}`}
                      >
                        {t("request.campaign.open")}
                      </Link>
                    ) : (
                      <span className="muted">{t("request.notExposed")}</span>
                    )}
                  </dd>
                </div>
                <div>
                  <dt>{t("request.campaign.status")}</dt>
                  <dd>
                    {data.campaign?.campaign.effective_status ||
                    data.campaign?.campaign.status ||
                    data.campaignStatus ? (
                      <StatusBadge
                        status={
                          data.campaign?.campaign.effective_status ??
                          data.campaign?.campaign.status ??
                          data.campaignStatus ??
                          "UNKNOWN"
                        }
                      />
                    ) : (
                      t("request.notExposed")
                    )}
                  </dd>
                </div>
                <div>
                  <dt>{t("request.campaign.authorization")}</dt>
                  <dd>
                    {data.campaign?.authorization
                      ? `${data.campaign.authorization.riskClass} · ${data.campaign.authorization.executionMode}${
                          data.campaign.authorization.provider ||
                          data.campaign.authorization.model
                            ? ` · ${data.campaign.authorization.provider}/${data.campaign.authorization.model}`
                            : ""
                        }`
                      : t("request.notExposed")}
                  </dd>
                </div>
                <div>
                  <dt>{t("request.campaign.runsCandidates")}</dt>
                  <dd>
                    {data.campaign
                      ? t("request.campaign.counts", {
                          runs: formatNumber(data.campaign.runs.length),
                          candidates: formatNumber(
                            data.campaign.candidates.length,
                          ),
                        })
                      : t("request.notExposed")}
                  </dd>
                </div>
              </dl>
            </Panel>
          )}

          {data.gateOutcome === "REPAIR_REQUIRED" && data.campaignId && (
            <Panel className="request-panel request-repair">
              <div className="section-heading">
                <h2>{t("request.repair.title")}</h2>
                <Badge tone="attention">{t("request.repair.badge")}</Badge>
              </div>
              <p className="muted">{t("request.repair.description")}</p>
              {!repairOpen ? (
                <Button
                  variant="secondary"
                  disabled={offline}
                  onClick={() => setRepairOpen(true)}
                >
                  {t("request.repair.open")}
                </Button>
              ) : (
                <form
                  className="request-cancel-form"
                  onSubmit={(event) => void repairFromCandidate(event)}
                >
                  <Textarea
                    label={t("request.repair.reason")}
                    value={repairReason}
                    onChange={(event) => setRepairReason(event.target.value)}
                    disabled={repairBusy || offline}
                  />
                  <Checkbox
                    label={t("request.repair.confirm")}
                    checked={repairConfirmed}
                    onChange={(event) =>
                      setRepairConfirmed(event.target.checked)
                    }
                    disabled={repairBusy || offline}
                  />
                  <div className="request-form-actions">
                    <Button
                      type="submit"
                      variant="primary"
                      disabled={!repairConfirmed || repairBusy || offline}
                    >
                      {repairBusy
                        ? t("request.repair.starting")
                        : t("request.repair.start")}
                    </Button>
                    <Button
                      type="button"
                      variant="secondary"
                      disabled={repairBusy}
                      onClick={() => setRepairOpen(false)}
                    >
                      {t("request.repair.cancel")}
                    </Button>
                  </div>
                  {repairError && (
                    <p className="ui-callout ui-callout--danger" role="alert">
                      {repairError}
                    </p>
                  )}
                </form>
              )}
            </Panel>
          )}

          <Panel className="request-panel">
            <div className="section-heading">
              <h2>{t("request.conversation.title")}</h2>
              <span className="muted">
                {t("request.conversation.count", {
                  count: formatNumber(data.messages.length),
                })}
              </span>
            </div>
            <p className="muted">
              {canFollowUp
                ? t("request.conversation.followUpDescription")
                : t("request.conversation.description")}
            </p>
            {data.parentRequestId && (
              <p className="muted request-follow-up-parent">
                {t("request.conversation.followUpFrom")}{" "}
                <Link
                  to={`/requests/${encodeURIComponent(data.parentRequestId)}`}
                >
                  {shortId(data.parentRequestId)}
                </Link>
              </p>
            )}
            {followUpRequest && (
              <div className="ui-callout request-follow-up" role="status">
                <p>
                  {t("request.conversation.followUpCreated")}{" "}
                  <IntakeStateBadge state={followUpRequest.state} />
                </p>
                <Link
                  className={buttonClass("secondary")}
                  to={`/requests/${encodeURIComponent(followUpRequest.id)}`}
                >
                  {t("request.conversation.openFollowUp")}
                </Link>
              </div>
            )}
            {data.messages.length === 0 ? (
              <EmptyState
                title={t("request.conversation.emptyTitle")}
                description={t("request.conversation.emptyDescription")}
              />
            ) : (
              <ol className="request-messages">
                {data.messages.map((message) => (
                  <li
                    key={message.id}
                    className={`request-message request-message--${message.role}`}
                  >
                    <div className="request-message-head">
                      <strong>{messageRoleLabel(message.role, t)}</strong>
                      <time dateTime={message.createdAt}>
                        {formatDate(message.createdAt)}
                      </time>
                    </div>
                    <p className="request-message-body">{message.body}</p>
                  </li>
                ))}
              </ol>
            )}
            {canMessage ? (
              <form
                className="request-message-form"
                onSubmit={(event) => void sendMessage(event)}
              >
                <Textarea
                  label={
                    canFollowUp
                      ? t("request.conversation.followUpLabel")
                      : t("request.conversation.messageLabel")
                  }
                  value={messageBody}
                  onChange={(event) => setMessageBody(event.target.value)}
                  placeholder={
                    canFollowUp
                      ? t("request.conversation.followUpPlaceholder")
                      : t("request.conversation.messagePlaceholder")
                  }
                  disabled={offline}
                />
                <div className="request-form-actions">
                  <Button
                    type="submit"
                    disabled={
                      busy !== null || offline || messageBody.trim() === ""
                    }
                  >
                    {busy === "message"
                      ? t("request.conversation.sending")
                      : canFollowUp
                        ? t("request.conversation.startFollowUp")
                        : t("request.conversation.send")}
                  </Button>
                </div>
              </form>
            ) : followUpRequest !== null ? (
              <p className="muted">
                {t("request.conversation.followUpExists")}
              </p>
            ) : data.plan !== null ? (
              <p className="muted">{t("request.conversation.frozen")}</p>
            ) : (
              <p className="muted">{t("request.conversation.terminal")}</p>
            )}
          </Panel>
        </div>

        <aside className="request-side" aria-label={t("request.details.aria")}>
          <Panel className="request-panel">
            <h2>{t("request.details.title")}</h2>
            <dl className="request-facts">
              <div>
                <dt>{t("request.details.request")}</dt>
                <dd>
                  <code>{shortId(data.id)}</code>
                </dd>
              </div>
              <div>
                <dt>{t("request.details.project")}</dt>
                <dd>{projectName ?? shortId(data.projectId)}</dd>
              </div>
              <div>
                <dt>{t("request.details.profile")}</dt>
                <dd>
                  {data.profile.name} ·{" "}
                  {t("request.details.revision", {
                    revision: data.profile.revision,
                  })}
                </dd>
              </div>
              <div>
                <dt>{t("request.details.created")}</dt>
                <dd>{formatDate(data.createdAt)}</dd>
              </div>
              <div>
                <dt>{t("request.details.updated")}</dt>
                <dd>{formatDate(data.updatedAt)}</dd>
              </div>
            </dl>
          </Panel>

          {canCancel && (
            <Panel className="request-panel">
              <h2>{t("request.cancel.title")}</h2>
              <p className="muted">{t("request.cancel.description")}</p>
              <Button
                variant="quiet"
                aria-expanded={cancelOpen}
                onClick={() => setCancelOpen((open) => !open)}
              >
                {cancelOpen
                  ? t("request.cancel.close")
                  : t("request.cancel.open")}
              </Button>
              {cancelOpen && (
                <form
                  className="request-cancel-form"
                  onSubmit={(event) => void cancelRequest(event)}
                >
                  <Textarea
                    label={t("request.cancel.reason")}
                    value={cancelReason}
                    onChange={(event) => setCancelReason(event.target.value)}
                    disabled={offline}
                    required
                  />
                  <Button
                    type="submit"
                    variant="danger"
                    disabled={
                      busy !== null || offline || cancelReason.trim() === ""
                    }
                  >
                    {busy === "cancel"
                      ? t("request.cancel.cancelling")
                      : t("request.cancel.confirm")}
                  </Button>
                </form>
              )}
            </Panel>
          )}
        </aside>
      </div>
    </div>
  );
}
