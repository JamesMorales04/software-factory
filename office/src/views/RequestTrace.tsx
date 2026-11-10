import { useCallback, useEffect, useRef, useState } from "react";
import { StatusBadge } from "../components/office/StatusBadge";
import { Badge, type BadgeTone } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  Panel,
} from "../components/ui/Surface";
import {
  CoreApiError,
  coreApi,
  type TraceAlert,
  type TraceAlertKind,
  type WorkflowTraceView,
} from "../coreApi";
import { shortId } from "../format";
import { type MessageKey, useI18n } from "../i18n";

const TRACE_EVENT_LIMIT = 200;
const TRACE_REFRESH_MS = 10_000;

/// Defense-in-depth display guard. Core already redacts payloads; the browser
/// refuses secret/prompt-shaped keys again so a Core regression cannot render a
/// raw credential or prompt through this view.
const SECRET_KEY_MARKERS = [
  "token",
  "password",
  "secret",
  "authorization",
  "apikey",
  "prompt",
  "content",
];

function isSecretPayloadKey(key: string): boolean {
  const normalized = key.toLowerCase().replace(/[^a-z0-9]/g, "");
  return SECRET_KEY_MARKERS.some((marker) => normalized.includes(marker));
}

const WORKFLOW_STATES = new Set([
  "SUBMITTED",
  "ASSESSING",
  "PLANNED",
  "PLAN_REVIEW",
  "FROZEN",
  "EXECUTING",
  "VERIFYING",
  "REVIEWING",
  "GATE",
  "REPAIRING",
  "REPLANNING",
  "CONVERGING",
  "ACCEPTED",
  "BLOCKED",
  "FAILED",
  "CANCELLED",
]);

const STEP_STATUSES = new Set([
  "PENDING",
  "RUNNING",
  "SUCCEEDED",
  "FAILED",
  "BLOCKED",
]);

const GATE_OUTCOMES = new Set([
  "ACCEPTED",
  "REPAIR_REQUIRED",
  "BLOCKED",
  "HUMAN_REQUIRED",
  "NOT_TESTED",
  "PASS",
  "FAIL",
  "CHANGES_REQUESTED",
]);

function normalizePayloadKey(key: string): string {
  return key.toLowerCase().replace(/[^a-z0-9]/g, "");
}

const OPERATIONAL_IDENTIFIER =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

function isStepId(value: string): boolean {
  return (
    value.startsWith("step-") && OPERATIONAL_IDENTIFIER.test(value.slice(5))
  );
}

function isWorkflowRunId(value: string): boolean {
  return (
    value.startsWith("workflow-") && OPERATIONAL_IDENTIFIER.test(value.slice(9))
  );
}

function isUuid(value: string): boolean {
  return OPERATIONAL_IDENTIFIER.test(value);
}

function operationalIdText(value: string | null): string {
  return value !== null && isStepId(value) ? shortId(value) : "[REDACTED]";
}

const CANONICAL_PAYLOAD_KEYS: Record<string, string> = {
  kind: "kind",
  state: "state",
  statefrom: "stateFrom",
  stateto: "stateTo",
  status: "status",
  outcome: "outcome",
  sequence: "sequence",
  stepid: "stepId",
  runid: "runId",
  requestid: "requestId",
  attempt: "attempt",
  count: "count",
  durationms: "durationMs",
  bytes: "bytes",
  sha256: "sha256",
  payloadsha256: "payloadSha256",
  terminal: "terminal",
  token: "token",
  password: "password",
  secret: "secret",
  authorization: "authorization",
  apikey: "api_key",
  prompt: "prompt",
  content: "content",
};

function displayPayloadKey(key: string): string {
  const canonical = CANONICAL_PAYLOAD_KEYS[normalizePayloadKey(key)];
  if (canonical) return canonical;
  if (/^redacted_[a-f0-9]{12}$/.test(key)) return key;
  return "[redacted]";
}

function isSafePayloadString(key: string, value: string): boolean {
  switch (normalizePayloadKey(key)) {
    case "state":
    case "statefrom":
    case "stateto":
      return WORKFLOW_STATES.has(value);
    case "status":
      return WORKFLOW_STATES.has(value) || STEP_STATUSES.has(value);
    case "outcome":
      return GATE_OUTCOMES.has(value);
    case "sha256":
    case "payloadsha256":
      return /^[a-f0-9]{64}$/.test(value);
    case "stepid":
      return isStepId(value);
    case "runid":
      return isWorkflowRunId(value);
    case "requestid":
      return isUuid(value);
    default:
      return false;
  }
}

function displayPayloadValue(key: string, value: unknown): string {
  if (isSecretPayloadKey(key)) return "[REDACTED]";
  if (typeof value === "string") {
    return isSafePayloadString(key, value) ? value : "[REDACTED]";
  }
  if (typeof value === "boolean" || typeof value === "number") {
    return String(value);
  }
  // Nested structures are not re-rendered in the browser; Core's redacted
  // scalar projection is the only display contract.
  return "[REDACTED]";
}

const ALERT_LABEL_KEYS: Record<TraceAlertKind, MessageKey> = {
  DUPLICATE_SEQUENCE: "request.trace.alert.DUPLICATE_SEQUENCE",
  MISSING_TRANSITION: "request.trace.alert.MISSING_TRANSITION",
  OPEN_STEP: "request.trace.alert.OPEN_STEP",
  TERMINAL_STATE: "request.trace.alert.TERMINAL_STATE",
  NON_TERMINAL_STATE: "request.trace.alert.NON_TERMINAL_STATE",
  TRACE_TRUNCATED: "request.trace.alert.TRACE_TRUNCATED",
  STEP_TRUNCATED: "request.trace.alert.STEP_TRUNCATED",
  REPLAY_REJECTED: "request.trace.alert.REPLAY_REJECTED",
  STATE_DIVERGENCE: "request.trace.alert.STATE_DIVERGENCE",
  NO_EVENTS: "request.trace.alert.NO_EVENTS",
  MALFORMED_EVENT: "request.trace.alert.MALFORMED_EVENT",
};

function errorMessage(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError
    ? cause.message
    : cause instanceof Error
      ? cause.message
      : fallback;
}

function alertTone(alert: TraceAlert): BadgeTone {
  if (alert.severity === "CRITICAL") return "danger";
  if (alert.severity === "WARNING") return "attention";
  return "info";
}

function alertDetail(
  alert: TraceAlert,
  t: ReturnType<typeof useI18n>["t"],
): string | null {
  switch (alert.kind) {
    case "DUPLICATE_SEQUENCE":
      return alert.sequence === null
        ? null
        : t("request.trace.alert.duplicateDetail", {
            sequence: alert.sequence,
            count: alert.occurrences ?? 0,
          });
    case "MISSING_TRANSITION":
      return alert.sequence === null
        ? null
        : t("request.trace.alert.missingTransitionDetail", {
            sequence: alert.sequence,
            expected: alert.expectedFrom ?? t("common.unknown"),
            observed: alert.observedFrom ?? t("common.unknown"),
          });
    case "OPEN_STEP":
      return alert.stepId === null
        ? null
        : t("request.trace.alert.openStepDetail", {
            stepId: operationalIdText(alert.stepId),
          });
    case "STATE_DIVERGENCE":
      return t("request.trace.alert.stateDivergenceDetail", {
        expected: alert.expectedState ?? t("common.unknown"),
        observed: alert.observedState ?? t("common.unknown"),
      });
    case "MALFORMED_EVENT":
      return alert.sequence === null
        ? null
        : t("request.trace.alert.malformedEventDetail", {
            sequence: alert.sequence,
          });
    case "TRACE_TRUNCATED":
      return t("request.trace.alert.truncatedDetail");
    case "STEP_TRUNCATED":
      return t("request.trace.alert.stepsTruncatedDetail");
    case "REPLAY_REJECTED":
      return t("request.trace.alert.rejectedDetail");
    case "NO_EVENTS":
      return t("request.trace.alert.noEventsDetail");
    case "TERMINAL_STATE":
      return t("request.trace.alert.terminalDetail");
    case "NON_TERMINAL_STATE":
      return t("request.trace.alert.nonTerminalDetail");
    default:
      return null;
  }
}

function payloadEntries(payload: unknown): Array<[string, string]> {
  if (payload === null || payload === undefined) return [];
  if (typeof payload === "object" && !Array.isArray(payload)) {
    return Object.entries(payload as Record<string, unknown>)
      .slice(0, 50)
      .map(([key, value]) => [
        displayPayloadKey(key),
        displayPayloadValue(key, value),
      ]);
  }
  return [["[redacted]", displayPayloadValue("value", payload)]];
}

export function RequestTrace({ requestId }: { requestId: string }) {
  const { t, formatDate, formatNumber } = useI18n();
  const [data, setData] = useState<WorkflowTraceView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [offline, setOffline] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const hasData = useRef(false);

  useEffect(() => {
    let active = true;
    // `reloadToken` is an explicit manual-refresh trigger; the effect body
    // reads it so dependency checking remains honest.
    void reloadToken;
    hasData.current = false;
    setData(null);
    setError(null);
    setOffline(null);
    setLoading(true);
    setLastLoadedAt(null);

    async function load(background: boolean) {
      try {
        const next = await coreApi.getRequestTrace(
          requestId,
          TRACE_EVENT_LIMIT,
        );
        if (!active) return;
        hasData.current = true;
        setData(next);
        setError(null);
        setOffline(null);
        setLastLoadedAt(new Date().toISOString());
      } catch (cause) {
        if (!active) return;
        const message = errorMessage(cause, t("request.trace.error"));
        if (background && hasData.current) {
          setOffline(message);
        } else {
          setError(message);
        }
      } finally {
        if (active) setLoading(false);
      }
    }

    void load(false);
    const timer = window.setInterval(() => void load(true), TRACE_REFRESH_MS);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [requestId, reloadToken, t]);

  const refresh = useCallback(() => {
    setReloadToken((value) => value + 1);
  }, []);

  if (loading && data === null && error === null) {
    return (
      <Panel className="request-panel request-trace" as="section">
        <div className="section-heading">
          <h2>{t("request.trace.title")}</h2>
        </div>
        <LoadingState label={t("request.trace.loading")} />
      </Panel>
    );
  }

  if (error !== null && data === null) {
    return (
      <Panel className="request-panel request-trace" as="section">
        <div className="section-heading">
          <h2>{t("request.trace.title")}</h2>
        </div>
        <ErrorState
          title={t("request.trace.errorTitle")}
          message={error}
          action={
            <Button onClick={refresh} disabled={loading}>
              {t("request.trace.retry")}
            </Button>
          }
        />
      </Panel>
    );
  }

  if (data === null) {
    return null;
  }

  const eventEntries = data.events;
  const run = data.run;
  const replayOutcome =
    data.replayError !== null
      ? "rejected"
      : data.trace === null
        ? "unknown"
        : data.terminal
          ? "terminal"
          : "inProgress";

  return (
    <Panel className="request-panel request-trace" as="section">
      <div className="section-heading">
        <h2>{t("request.trace.title")}</h2>
        <div className="request-trace-head-actions">
          {lastLoadedAt && (
            <span className="muted">
              {t("request.trace.updated", {
                date: formatDate(lastLoadedAt),
              })}
            </span>
          )}
          <Button
            size="sm"
            variant="quiet"
            busy={loading}
            disabled={loading}
            onClick={refresh}
          >
            {t("request.trace.refresh")}
          </Button>
        </div>
      </div>
      <p className="muted">{t("request.trace.description")}</p>

      {offline && (
        <p className="ui-callout ui-callout--attention" role="status">
          {t("request.trace.offline", {
            error: offline,
            loadedAt: formatDate(lastLoadedAt ?? run?.updatedAt ?? ""),
          })}{" "}
          <Button size="sm" onClick={refresh} disabled={loading}>
            {t("request.trace.retryNow")}
          </Button>
        </p>
      )}

      {data.replayError && (
        <div className="ui-callout ui-callout--danger" role="alert">
          <p>
            <strong>{t("request.trace.replayRejected")}</strong>{" "}
            {data.replayError}
          </p>
          <p className="muted">{t("request.trace.replayRejectedHint")}</p>
        </div>
      )}

      {data.truncated && (
        <p className="ui-callout ui-callout--attention" role="status">
          {t("request.trace.truncated", {
            limit: formatNumber(data.eventLimit),
          })}
        </p>
      )}
      {data.stepsTruncated && (
        <p className="ui-callout ui-callout--attention" role="status">
          {t("request.trace.stepsTruncated", {
            limit: formatNumber(data.stepLimit),
          })}
        </p>
      )}

      {run === null ? (
        <EmptyState
          title={t("request.trace.emptyTitle")}
          description={t("request.trace.emptyDescription")}
        />
      ) : (
        <>
          <dl className="request-facts">
            <div>
              <dt>{t("request.trace.run")}</dt>
              <dd>
                <code title={run.id}>{shortId(run.id)}</code>
              </dd>
            </div>
            <div>
              <dt>{t("request.trace.state")}</dt>
              <dd>
                <StatusBadge status={run.state} />
              </dd>
            </div>
            <div>
              <dt>{t("request.trace.revision")}</dt>
              <dd>{formatNumber(run.revision)}</dd>
            </div>
            <div>
              <dt>{t("request.trace.finalState")}</dt>
              <dd>
                {data.trace?.finalState ? (
                  <StatusBadge status={data.trace.finalState} />
                ) : (
                  <span className="muted">{t("common.unknown")}</span>
                )}
              </dd>
            </div>
            <div>
              <dt>{t("request.trace.outcome")}</dt>
              <dd>
                <Badge
                  tone={
                    replayOutcome === "rejected"
                      ? "danger"
                      : replayOutcome === "unknown"
                        ? "attention"
                        : replayOutcome === "terminal"
                          ? "success"
                          : "info"
                  }
                >
                  {replayOutcome === "rejected"
                    ? t("request.trace.outcomeRejected")
                    : replayOutcome === "unknown"
                      ? t("request.trace.outcomeUnknown")
                      : replayOutcome === "terminal"
                        ? t("request.trace.terminal")
                        : t("request.trace.inProgress")}
                </Badge>
              </dd>
            </div>
            <div>
              <dt>{t("request.trace.eventCount")}</dt>
              <dd>
                {t("request.trace.eventCountValue", {
                  events: formatNumber(data.events.length),
                  limit: formatNumber(data.eventLimit),
                })}
              </dd>
            </div>
          </dl>

          <div className="request-trace-section">
            <h3>{t("request.trace.alertsTitle")}</h3>
            {data.alerts.length === 0 ? (
              <p className="muted">{t("request.trace.alertsEmpty")}</p>
            ) : (
              <ul
                className="request-trace-alerts"
                aria-label={t("request.trace.alertsAria")}
              >
                {data.alerts.map((alert, index) => {
                  const detail = alertDetail(alert, t);
                  return (
                    <li
                      key={`${alert.kind}-${alert.sequence ?? alert.stepId ?? index}`}
                      className="request-trace-alert"
                    >
                      <Badge tone={alertTone(alert)}>
                        {t(ALERT_LABEL_KEYS[alert.kind])}
                      </Badge>
                      {detail && <span>{detail}</span>}
                      {alert.sequence !== null && (
                        <code className="muted">
                          {t("request.trace.sequence", {
                            sequence: formatNumber(alert.sequence),
                          })}
                        </code>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
          </div>

          <div className="request-trace-section">
            <div className="section-heading">
              <h3>{t("request.trace.stepsTitle")}</h3>
              <span className="muted">
                {t("request.trace.stepsCountValue", {
                  count: formatNumber(data.steps.length),
                  limit: formatNumber(data.stepLimit),
                })}
              </span>
            </div>
            {data.steps.length === 0 ? (
              <p className="muted">{t("request.trace.stepsEmpty")}</p>
            ) : (
              <ol className="request-trace-steps">
                {data.steps.map((step) => (
                  <li key={step.stepId} className="request-trace-step">
                    <div className="request-trace-step-head">
                      <code title={operationalIdText(step.stepId)}>
                        {operationalIdText(step.stepId)}
                      </code>
                      <StatusBadge status={step.status} />
                      {step.open && (
                        <Badge tone="attention">
                          {t("request.trace.stepOpen")}
                        </Badge>
                      )}
                    </div>
                    <dl className="request-trace-step-facts">
                      <div>
                        <dt>{t("request.trace.stepKind")}</dt>
                        <dd>{step.kind}</dd>
                      </div>
                      <div>
                        <dt>{t("request.trace.stepAttempt")}</dt>
                        <dd>{formatNumber(step.attempt)}</dd>
                      </div>
                      <div>
                        <dt>{t("request.trace.stepUpdated")}</dt>
                        <dd>{formatDate(step.updatedAt)}</dd>
                      </div>
                      <div>
                        <dt>{t("request.trace.stepSequence")}</dt>
                        <dd>
                          {step.lastSequence === null
                            ? t("common.dash")
                            : formatNumber(step.lastSequence)}
                        </dd>
                      </div>
                    </dl>
                  </li>
                ))}
              </ol>
            )}
          </div>

          <div className="request-trace-section">
            <div className="section-heading">
              <h3>{t("request.trace.eventLogTitle")}</h3>
              <span className="muted">{t("request.trace.eventLogHint")}</span>
            </div>
            {eventEntries.length === 0 ? (
              <p className="muted">{t("request.trace.eventLogEmpty")}</p>
            ) : (
              <section
                className="request-trace-events"
                aria-label={t("request.trace.eventLogAria")}
              >
                <ol>
                  {eventEntries.map((event) => {
                    const entries = payloadEntries(event.payload);
                    return (
                      <li key={event.sequence} className="request-trace-event">
                        <details>
                          <summary>
                            <span className="request-trace-event-sequence">
                              #{formatNumber(event.sequence)}
                            </span>
                            <code>{event.kind}</code>
                            {event.stepId && (
                              <code
                                className="request-trace-event-step"
                                title={operationalIdText(event.stepId)}
                              >
                                {operationalIdText(event.stepId)}
                              </code>
                            )}
                            {event.stateFrom && event.stateTo && (
                              <span className="request-trace-event-transition">
                                {event.stateFrom} → {event.stateTo}
                              </span>
                            )}
                            <time dateTime={event.recordedAt}>
                              {formatDate(event.recordedAt)}
                            </time>
                          </summary>
                          {entries.length === 0 ? (
                            <p className="muted">
                              {t("request.trace.noPayload")}
                            </p>
                          ) : (
                            <dl className="request-trace-payload">
                              {entries.map(([key, value]) => (
                                <div key={key}>
                                  <dt>
                                    <code>{key}</code>
                                  </dt>
                                  <dd>{value}</dd>
                                </div>
                              ))}
                            </dl>
                          )}
                          <p className="muted request-trace-hash">
                            {t("request.trace.payloadHash", {
                              hash: event.payloadSha256,
                            })}
                          </p>
                        </details>
                      </li>
                    );
                  })}
                </ol>
              </section>
            )}
          </div>
        </>
      )}
    </Panel>
  );
}
