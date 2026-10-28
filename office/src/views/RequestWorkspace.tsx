import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
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
import { formatTimestamp, shortId } from "../format";
import { RequestAssessment } from "./RequestAssessment";
import { RequestPlan } from "./RequestPlan";
import {
  IntakeModeBadge,
  IntakeStateBadge,
  isTerminalIntakeState,
} from "./RequestState";

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

function messageRoleLabel(role: string): string {
  if (role === "operator") return "Operator";
  if (role === "system") return "System";
  return role.replaceAll("_", " ");
}

export function RequestWorkspace() {
  const { requestId = "" } = useParams<{ requestId: string }>();
  const [data, setData] = useState<IntakeRequestProjection | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notFound, setNotFound] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [rationale, setRationale] = useState("");
  const [approvedScope, setApprovedScope] = useState(false);
  const [messageBody, setMessageBody] = useState("");
  const [cancelOpen, setCancelOpen] = useState(false);
  const [cancelReason, setCancelReason] = useState("");
  const messageKey = useRef(newIdempotencyKey());

  const load = useCallback(
    async (background = false) => {
      try {
        const next = await coreApi.getIntakeRequest(requestId);
        setData(next);
        setLoadError(null);
        setNotFound(false);
        setConnectionError(null);
        setLastLoadedAt(new Date().toISOString());
      } catch (cause) {
        const message = errorMessage(cause, "Failed to load the request.");
        if (background) {
          setConnectionError(message);
        } else {
          setLoadError(message);
          setNotFound(isNotFound(cause));
        }
      }
    },
    [requestId],
  );

  useEffect(() => {
    setData(null);
    setLoadError(null);
    setNotFound(false);
    setConnectionError(null);
    setLastLoadedAt(null);
    setActionError(null);
    setNotice(null);
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
      setActionError(
        errorMessage(cause, "The Core operation could not be completed."),
      );
      return false;
    } finally {
      setBusy(null);
    }
  }

  if (data === null) {
    if (loadError !== null) {
      return (
        <ErrorState
          title={notFound ? "Request not found" : "Unable to load request"}
          message={loadError}
          action={
            <div className="request-error-actions">
              <Button onClick={() => void load(false)}>Retry</Button>
              <Link className={buttonClass("quiet")} to="/">
                Back to Home
              </Link>
            </div>
          }
        />
      );
    }
    return <LoadingState label="Loading request…" />;
  }

  const terminal = isTerminalIntakeState(data.state);
  const canMessage = !terminal;
  const canCancel = !terminal && data.state !== "EXECUTING";
  const authorizationPending = ![
    "EXECUTING",
    "COMPLETED",
    "CANCELLED",
  ].includes(data.state);
  const authorizationAvailable = data.authorization.available;
  const offline = connectionError !== null;
  const approvalReady =
    data.state === "AWAITING_APPROVAL" && data.plan !== null;
  const approveDisabled =
    busy !== null ||
    offline ||
    !approvalReady ||
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
      "Core recorded the assessment. Review the proposal and frozen plan before authorizing.",
    );
  }

  async function freezePlan() {
    if (!data) return;
    await runAction(
      "freeze",
      async () => {
        await coreApi.freezeLeadRequest(data.leadRequestId);
      },
      "Core froze the plan. Approval now requires the exact plan hash shown below.",
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
          actor: "office-operator",
          rationale: rationale.trim(),
        });
      },
      "Core recorded the approval. Execution state is shown from the refreshed projection.",
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
      "Message recorded by Core.",
    );
    if (ok) {
      messageKey.current = newIdempotencyKey();
      setMessageBody("");
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
      "Core recorded the request cancellation. This does not cancel an authorized Campaign.",
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
        description="Operator request, Core-derived assessment and the exact plan and authority state behind execution."
        meta={
          <>
            <span>
              Request <code>{shortId(data.id)}</code>
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              Project <code>{shortId(data.projectId)}</code>
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              Profile {data.profile.name} r{data.profile.revision}
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>Updated {formatTimestamp(data.updatedAt)}</span>
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
                Open Campaign
              </Link>
            )}
            <Link className={buttonClass("quiet")} to="/">
              Back to Home
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
            Core could not be refreshed: {connectionError}. Showing the last
            response loaded {formatTimestamp(lastLoadedAt ?? data.updatedAt)}.
            The request was last updated {formatTimestamp(data.updatedAt)}.
            Actions are paused until the next successful refresh.
          </p>
          <Button size="sm" onClick={() => void load(true)}>
            Retry now
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
            <strong>Authorization required.</strong> Core holds the frozen plan.
            Execution starts only after an explicit operator approval; the
            approval cannot widen the plan, budget, route or paths.
          </p>
        )}
      {authorizationPending &&
        data.authorization.required &&
        !data.authorization.available && (
          <p
            className="ui-callout ui-callout--danger request-callout"
            role="alert"
          >
            <strong>Authorization unavailable.</strong>{" "}
            {data.authorization.reason ??
              "Core did not provide a reason for this state."}{" "}
            The request cannot start until Core can derive the authorization.
          </p>
        )}
      {authorizationPending && !data.authorization.required && (
        <p className="ui-callout ui-callout--info request-callout">
          Core reports that no additional authorization is required for this
          request. Campaign lifecycle state remains authoritative.
        </p>
      )}

      {actionError && (
        <p className="ui-callout ui-callout--danger" role="alert">
          {actionError}
        </p>
      )}
      {notice && (
        <p className="notice" role="status" aria-live="polite">
          {notice}
        </p>
      )}

      <div className="request-layout">
        <div className="request-column">
          {(data.state === "SUBMITTED" ||
            data.state === "ASSESSING" ||
            data.state === "PLAN_READY" ||
            data.state === "AWAITING_APPROVAL") && (
            <Panel className="request-panel request-next-step">
              <div className="section-heading">
                <h2>Next step</h2>
                <span className="muted">Core-owned operation</span>
              </div>
              {data.state === "SUBMITTED" && (
                <>
                  <p className="muted">
                    Run the bounded assessment with the frozen profile&apos;s
                    Lead route. If Core has no routable model, it returns an
                    explicit unavailable error rather than falling back.
                  </p>
                  <Button
                    variant="primary"
                    disabled={busy !== null || offline}
                    onClick={() => void assess()}
                  >
                    {busy === "assess" ? "Assessing…" : "Run assessment"}
                  </Button>
                </>
              )}
              {data.state === "ASSESSING" && (
                <>
                  <p className="muted" role="status">
                    Core reports an assessment in progress. This view does not
                    stream progress; it refreshes from Core.
                  </p>
                  <Button
                    disabled={busy !== null}
                    onClick={() => void load(true)}
                  >
                    Refresh request
                  </Button>
                </>
              )}
              {data.state === "PLAN_READY" &&
                (data.proposal ? (
                  <>
                    <p className="muted">
                      Core proposed a plan but has not exposed a frozen plan
                      hash. Freeze the existing Lead plan record before
                      approval.
                    </p>
                    <Button
                      variant="primary"
                      disabled={
                        busy !== null || offline || data.leadRequestId === ""
                      }
                      onClick={() => void freezePlan()}
                    >
                      {busy === "freeze" ? "Freezing…" : "Freeze plan"}
                    </Button>
                  </>
                ) : (
                  <p className="muted">
                    Core reports a ready plan without a recorded proposal.
                    Refresh the request or inspect the Lead record.
                  </p>
                ))}
              {data.state === "AWAITING_APPROVAL" &&
                (data.plan === null ? (
                  <p className="ui-callout ui-callout--danger">
                    Core reports awaiting approval without a frozen plan.
                    Approval is unavailable; refresh or inspect the Lead record.
                  </p>
                ) : (
                  <form
                    className="request-approval"
                    onSubmit={(event) => void approve(event)}
                  >
                    <p className="muted">
                      Approving composes the existing Lead start and Campaign
                      authorization operations. Core derives the implementer
                      route and bounded authority from the exact frozen profile
                      revision.
                    </p>
                    <dl className="request-facts">
                      <div>
                        <dt>Plan hash</dt>
                        <dd>
                          <code title={data.plan.plan_hash}>
                            {data.plan.plan_hash}
                          </code>
                        </dd>
                      </div>
                      <div>
                        <dt>Profile revision</dt>
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
                          "Core cannot derive an authorization for this plan."}
                      </p>
                    )}
                    <Textarea
                      label="Approval rationale"
                      value={rationale}
                      onChange={(event) => setRationale(event.target.value)}
                      disabled={!authorizationAvailable || offline}
                      help="Recorded with the authorization. It does not alter the frozen plan."
                      required
                    />
                    <Checkbox
                      label="I reviewed the frozen scope, route, budget and acceptance plan."
                      checked={approvedScope}
                      onChange={(event) =>
                        setApprovedScope(event.target.checked)
                      }
                      disabled={!authorizationAvailable || offline}
                    />
                    <div className="request-form-actions">
                      <Button
                        type="submit"
                        variant="primary"
                        disabled={approveDisabled}
                      >
                        {busy === "approve"
                          ? "Recording approval…"
                          : "Approve frozen plan and start"}
                      </Button>
                      {!authorizationAvailable && (
                        <span className="muted">
                          Approval is disabled until Core can derive the
                          authorization.
                        </span>
                      )}
                    </div>
                  </form>
                ))}
            </Panel>
          )}

          {data.state === "EXECUTING" && (
            <Panel className="request-panel">
              <h2>Execution</h2>
              <p className="muted">
                Core reports an authorized Campaign. Campaign, Run, candidate
                and Quality Gate state remain separate facts and are shown in
                the Campaign workspace.
              </p>
              {data.campaignId ? (
                <Link
                  className={buttonClass("primary")}
                  to={`/campaigns/${encodeURIComponent(data.campaignId)}`}
                >
                  Open Campaign {shortId(data.campaignId)}
                </Link>
              ) : (
                <p className="ui-callout ui-callout--attention">
                  Core reports execution without a Campaign id in this
                  projection. Refresh or inspect Campaigns.
                </p>
              )}
            </Panel>
          )}

          {data.state === "BLOCKED" && (
            <Panel className="request-panel">
              <h2>Blocked request</h2>
              <p className="muted">
                Core blocked this request. A new request is required if the
                cause is outside the frozen plan; messages can preserve the
                operator context.
              </p>
              <Link className={buttonClass("secondary")} to="/">
                Start a new request
              </Link>
            </Panel>
          )}

          {data.state === "FAILED" && (
            <Panel className="request-panel">
              <h2>Failed request</h2>
              <p className="muted">
                Core recorded a failure. Review the state detail above; Office
                does not retry paid or externally effectful work silently.
              </p>
              <Link className={buttonClass("secondary")} to="/">
                Start a new request
              </Link>
            </Panel>
          )}

          {data.state === "COMPLETED" && (
            <Panel className="request-panel">
              <h2>Completed</h2>
              <p className="muted">
                Core reports this request as completed. Quality Gate acceptance
                is only shown where the exact candidate has an ACCEPTED
                decision.
              </p>
              {data.campaignId && (
                <Link
                  className={buttonClass("secondary")}
                  to={`/campaigns/${encodeURIComponent(data.campaignId)}`}
                >
                  Open Campaign
                </Link>
              )}
            </Panel>
          )}

          {data.state === "CANCELLED" && (
            <Panel className="request-panel">
              <h2>Cancelled request</h2>
              <p className="muted">
                Core recorded an intake-only cancellation. An authorized
                Campaign, if any, has its own cancellation state.
              </p>
            </Panel>
          )}

          <RequestAssessment assessment={data.assessment} />
          <RequestPlan proposal={data.proposal} plan={data.plan} />

          {(data.campaignId || data.campaign) && (
            <Panel className="request-panel">
              <div className="section-heading">
                <h2>Campaign</h2>
                {data.gateOutcome && (
                  <Badge
                    tone={data.gateOutcome === "ACCEPTED" ? "success" : "muted"}
                  >
                    Gate {data.gateOutcome}
                  </Badge>
                )}
              </div>
              <dl className="request-facts">
                <div>
                  <dt>Campaign</dt>
                  <dd>
                    {data.campaignId ? (
                      <Link
                        to={`/campaigns/${encodeURIComponent(data.campaignId)}`}
                      >
                        <code>{shortId(data.campaignId)}</code>
                      </Link>
                    ) : (
                      <span className="muted">Not exposed</span>
                    )}
                  </dd>
                </div>
                <div>
                  <dt>Status</dt>
                  <dd>
                    {data.campaign?.campaign.status ??
                      data.campaignStatus ??
                      "Not exposed"}
                  </dd>
                </div>
                <div>
                  <dt>Authorization</dt>
                  <dd>
                    {data.campaign?.authorization
                      ? `${data.campaign.authorization.risk_class} · ${data.campaign.authorization.execution_mode}`
                      : "Not exposed"}
                  </dd>
                </div>
                <div>
                  <dt>Runs / candidates</dt>
                  <dd>
                    {data.campaign
                      ? `${data.campaign.runs.length} / ${data.campaign.candidates.length}`
                      : "Not exposed"}
                  </dd>
                </div>
              </dl>
            </Panel>
          )}

          <Panel className="request-panel">
            <div className="section-heading">
              <h2>Conversation</h2>
              <span className="muted">{data.messages.length} message(s)</span>
            </div>
            <p className="muted">
              Durable operator context. Messages never mutate a frozen plan; a
              material amendment requires a new request.
            </p>
            {data.messages.length === 0 ? (
              <EmptyState
                title="No messages yet"
                description="Use the composer to add durable context for this request."
              />
            ) : (
              <ol className="request-messages">
                {data.messages.map((message) => (
                  <li
                    key={message.id}
                    className={`request-message request-message--${message.role}`}
                  >
                    <div className="request-message-head">
                      <strong>{messageRoleLabel(message.role)}</strong>
                      <time dateTime={message.createdAt}>
                        {formatTimestamp(message.createdAt)}
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
                  label="Message the Factory"
                  value={messageBody}
                  onChange={(event) => setMessageBody(event.target.value)}
                  placeholder="Add operator context or a question"
                  disabled={offline}
                />
                <div className="request-form-actions">
                  <Button
                    type="submit"
                    disabled={
                      busy !== null || offline || messageBody.trim() === ""
                    }
                  >
                    {busy === "message" ? "Sending…" : "Send message"}
                  </Button>
                </div>
              </form>
            ) : (
              <p className="muted">
                Core reports this request as terminal, so new messages are not
                offered here.
              </p>
            )}
          </Panel>
        </div>

        <aside className="request-side" aria-label="Request details">
          <Panel className="request-panel">
            <h2>Request details</h2>
            <dl className="request-facts">
              <div>
                <dt>Request</dt>
                <dd>
                  <code>{shortId(data.id)}</code>
                </dd>
              </div>
              <div>
                <dt>Project</dt>
                <dd>
                  <code>{shortId(data.projectId)}</code>
                </dd>
              </div>
              <div>
                <dt>Profile</dt>
                <dd>
                  {data.profile.name} r{data.profile.revision}
                  <div className="muted">
                    <code title={data.profile.revisionId}>
                      {shortId(data.profile.revisionId, 12)}
                    </code>
                  </div>
                </dd>
              </div>
              <div>
                <dt>Action</dt>
                <dd>
                  <Link to={`/actions/${encodeURIComponent(data.actionId)}`}>
                    <code>{shortId(data.actionId)}</code>
                  </Link>
                </dd>
              </div>
              <div>
                <dt>Delegation</dt>
                <dd>
                  <code>{shortId(data.delegationId)}</code>
                </dd>
              </div>
              <div>
                <dt>Lead request</dt>
                <dd>
                  <Link to="/lead">
                    <code>{shortId(data.leadRequestId)}</code>
                  </Link>
                </dd>
              </div>
              <div>
                <dt>Created</dt>
                <dd>{formatTimestamp(data.createdAt)}</dd>
              </div>
              <div>
                <dt>Updated</dt>
                <dd>{formatTimestamp(data.updatedAt)}</dd>
              </div>
            </dl>
          </Panel>

          {canCancel && (
            <Panel className="request-panel">
              <h2>Cancel request</h2>
              <p className="muted">
                Records an intake-only cancellation. It does not cancel an
                authorized Campaign.
              </p>
              <Button
                variant="quiet"
                aria-expanded={cancelOpen}
                onClick={() => setCancelOpen((open) => !open)}
              >
                {cancelOpen ? "Close cancellation" : "Cancel this request…"}
              </Button>
              {cancelOpen && (
                <form
                  className="request-cancel-form"
                  onSubmit={(event) => void cancelRequest(event)}
                >
                  <Textarea
                    label="Cancellation reason"
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
                    {busy === "cancel" ? "Cancelling…" : "Confirm cancellation"}
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
