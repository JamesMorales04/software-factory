import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { ModelCallResolution } from "../components/office/ModelCallResolution";
import { ParticipantSessionList } from "../components/office/ParticipantSessionList";
import { StatusBadge } from "../components/office/StatusBadge";
import { Button, buttonClass } from "../components/ui/Button";
import { Input, Select } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import { type TabItem, Tabs } from "../components/ui/Tabs";
import {
  CoreApiError,
  coreApi,
  type FactorySettings,
  type Health,
  type ModelCallRecord,
  type ObservabilityStatus,
  type ProviderSession,
  type ProviderTurnBudget,
  type WorkerSummary,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

const TABS = ["overview", "sessions", "model-calls", "diagnostics"] as const;
type RuntimeTab = (typeof TABS)[number];

function resolveTab(value: string | null): RuntimeTab {
  return TABS.includes(value as RuntimeTab)
    ? (value as RuntimeTab)
    : "overview";
}

const UNRESOLVED_SESSION_STATES = [
  "INTENT",
  "ACTIVE",
  "CANCEL_REQUESTED",
  "ORPHANED",
];
function isUnresolvedSession(session: ProviderSession): boolean {
  return UNRESOLVED_SESSION_STATES.includes(session.state);
}

function Metric({
  label,
  value,
  detail,
  tone,
}: {
  label: string;
  value: string | number;
  detail?: string;
  tone?: "success" | "attention" | "danger" | "info";
}) {
  return (
    <Panel className="overview-metric">
      <p className="overview-metric-label">{label}</p>
      <p
        className={`overview-metric-value ${tone ? `overview-metric-value--${tone}` : ""}`}
      >
        {value}
      </p>
      {detail && <p className="overview-metric-detail">{detail}</p>}
    </Panel>
  );
}

function sessionCounts(sessions: ProviderSession[]): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const session of sessions) {
    counts[session.state] = (counts[session.state] ?? 0) + 1;
  }
  return counts;
}

function callCounts(calls: ModelCallRecord[]): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const call of calls) {
    counts[call.state] = (counts[call.state] ?? 0) + 1;
  }
  return counts;
}

export function Runtime() {
  const { t, formatDate, formatNumber, formatMicros } = useI18n();
  const [searchParams, setSearchParams] = useSearchParams();
  const tab = resolveTab(searchParams.get("tab"));
  const [sessions, setSessions] = useState<ProviderSession[]>([]);
  const [modelCalls, setModelCalls] = useState<ModelCallRecord[]>([]);
  const [unresolvedCalls, setUnresolvedCalls] = useState<ModelCallRecord[]>([]);
  const [observability, setObservability] =
    useState<ObservabilityStatus | null>(null);
  const [settings, setSettings] = useState<FactorySettings | null>(null);
  const [health, setHealth] = useState<Health | null>(null);
  const [workers, setWorkers] = useState<WorkerSummary[]>([]);
  const [turnBudget, setTurnBudget] = useState<ProviderTurnBudget | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const [sessionState, setSessionState] = useState("all");
  const [sessionUsage, setSessionUsage] = useState("all");
  const [sessionCleanup, setSessionCleanup] = useState("all");
  const [sessionWorker, setSessionWorker] = useState("");
  const [callRole, setCallRole] = useState("all");
  const [callState, setCallState] = useState("all");
  const [callUsage, setCallUsage] = useState("all");

  const selectTab = (next: string) => {
    const params = new URLSearchParams(searchParams);
    params.set("tab", resolveTab(next));
    setSearchParams(params);
  };

  const load = useCallback(async () => {
    const [
      nextHealth,
      nextWorkers,
      nextSessions,
      recentCalls,
      unresolved,
      telemetry,
      runtimeSettings,
      nextTurnBudget,
    ] = await Promise.all([
      coreApi.health(),
      coreApi.listWorkers(),
      coreApi.listProviderSessions(),
      coreApi.listModelCalls(200),
      coreApi.listUnresolvedModelCalls(),
      coreApi.getObservability(),
      coreApi.getRuntimeSettings(),
      coreApi.getProviderTurnBudget(),
    ]);
    setHealth(nextHealth);
    setWorkers(nextWorkers);
    setSessions(nextSessions);
    setModelCalls(recentCalls);
    setUnresolvedCalls(unresolved);
    setObservability(telemetry);
    setSettings(runtimeSettings);
    setTurnBudget(nextTurnBudget);
    setLastLoadedAt(new Date().toISOString());
    setLoaded(true);
    setError(null);
  }, []);

  useEffect(() => {
    let active = true;
    async function refresh() {
      try {
        await load();
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : t("runtime.error.load"),
          );
        }
      }
    }
    void refresh();
    const timer = setInterval(() => void refresh(), 5000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [load, t]);

  const allCalls = useMemo(() => {
    const byId = new Map<string, ModelCallRecord>();
    for (const call of modelCalls) byId.set(call.reservation_id, call);
    for (const call of unresolvedCalls) byId.set(call.reservation_id, call);
    return [...byId.values()].sort((left, right) =>
      right.updated_at.localeCompare(left.updated_at),
    );
  }, [modelCalls, unresolvedCalls]);

  const unresolvedSessions = sessions.filter(isUnresolvedSession);
  const measuredSessions = sessions.filter((session) => session.usage_known);
  const unmeasuredSessions = sessions.filter((session) => !session.usage_known);
  const measuredCalls = allCalls.filter((call) => call.usage_known);
  const measuredSessionCost = measuredSessions.reduce(
    (sum, session) => sum + session.cost_micros,
    0,
  );
  const measuredCallCost = measuredCalls.reduce(
    (sum, call) => sum + call.cost_micros,
    0,
  );
  const modelReservationCeiling = unresolvedCalls.reduce(
    (sum, call) => sum + call.max_cost_micros,
    0,
  );
  const reservationCeiling =
    modelReservationCeiling + (turnBudget?.ceiling_micros ?? 0);
  const measuredCost = measuredSessionCost + measuredCallCost;

  const sessionStates = ["all", ...Object.keys(sessionCounts(sessions)).sort()];
  const filteredSessions = sessions.filter((session) => {
    if (sessionState !== "all" && session.state !== sessionState) return false;
    if (sessionUsage === "measured" && !session.usage_known) return false;
    if (sessionUsage === "unmeasured" && session.usage_known) return false;
    if (sessionCleanup === "recorded" && !session.cleanup_receipt) return false;
    if (sessionCleanup === "missing" && session.cleanup_receipt) return false;
    if (
      sessionWorker.trim() !== "" &&
      !`${session.worker_id} ${session.provider} ${session.model}`
        .toLowerCase()
        .includes(sessionWorker.trim().toLowerCase())
    ) {
      return false;
    }
    return true;
  });

  const callStates = ["all", ...Object.keys(callCounts(allCalls)).sort()];
  const filteredCalls = allCalls.filter((call) => {
    if (callRole !== "all" && call.role !== callRole) return false;
    if (callState !== "all" && call.state !== callState) return false;
    if (callUsage === "measured" && !call.usage_known) return false;
    if (callUsage === "unmeasured" && call.usage_known) return false;
    return true;
  });

  if (error && !loaded) {
    return (
      <ErrorState
        title={t("common.errorTitle")}
        message={error}
        action={
          <Button onClick={() => window.location.reload()}>
            {t("common.retry")}
          </Button>
        }
      />
    );
  }
  if (!loaded) return <LoadingState label={t("runtime.loading")} />;

  const tabs: TabItem[] = [
    { id: "overview", label: t("runtime.tabs.overview") },
    {
      id: "sessions",
      label: t("runtime.tabs.sessions"),
      count: sessions.length,
    },
    {
      id: "model-calls",
      label: t("runtime.tabs.modelCalls"),
      count: allCalls.length,
    },
    { id: "diagnostics", label: t("runtime.tabs.diagnostics") },
  ];

  return (
    <div className="runtime-workspace">
      <PageHeader
        title={t("runtime.title")}
        description={t("runtime.description")}
        meta={
          <>
            <StatusBadge
              status={health?.healthy ? "HEALTHY" : "UNHEALTHY"}
              label={
                health?.healthy
                  ? t("runtime.meta.coreHealthy")
                  : t("runtime.meta.coreUnavailable")
              }
            />
            {health && (
              <span>
                {t("runtime.meta.coreVersion", { version: health.version })}
              </span>
            )}
            <span>
              {t("runtime.meta.fetched", {
                date: lastLoadedAt
                  ? formatDate(lastLoadedAt)
                  : t("common.dash"),
              })}
            </span>
            <span>{t("runtime.meta.workers", { count: workers.length })}</span>
          </>
        }
        actions={
          <>
            <Link className={buttonClass("secondary", "sm")} to="/inbox">
              {t("runtime.actions.inbox")}
            </Link>
            <Link className={buttonClass("quiet", "sm")} to="/settings">
              {t("runtime.actions.settings")}
            </Link>
          </>
        }
      />
      {error && (
        <div className="ui-callout ui-callout--danger" role="alert">
          {error} {t("runtime.staleSuffix")}
        </div>
      )}

      <Tabs
        items={tabs}
        value={tab}
        onChange={selectTab}
        label={t("runtime.tabs.label")}
      />

      {tab === "overview" && (
        <div
          id="panel-overview"
          role="tabpanel"
          aria-labelledby="tab-overview"
          className="campaign-tab-panel"
        >
          <section
            className="overview-metrics"
            aria-label={t("runtime.metrics.summaryAria")}
          >
            <Metric
              label={t("runtime.metrics.spend.label")}
              value={formatMicros(measuredCost)}
              detail={t("runtime.metrics.spend.detail", {
                sessions: formatNumber(measuredSessions.length),
                calls: formatNumber(measuredCalls.length),
              })}
            />
            <Metric
              label={t("runtime.metrics.ceiling.label")}
              value={formatMicros(reservationCeiling)}
              detail={t("runtime.metrics.ceiling.detail", {
                calls: formatNumber(unresolvedCalls.length),
                turns: formatNumber(turnBudget?.reserved_turns ?? 0),
              })}
              tone="attention"
            />
            <Metric
              label={t("runtime.metrics.unresolved.label")}
              value={formatNumber(unresolvedCalls.length)}
              detail={t("runtime.metrics.unresolved.detail")}
              tone={unresolvedCalls.length > 0 ? "attention" : undefined}
            />
            <Metric
              label={t("runtime.metrics.openSessions.label")}
              value={formatNumber(unresolvedSessions.length)}
              detail={t("runtime.metrics.openSessions.detail", {
                total: formatNumber(sessions.length),
              })}
              tone={unresolvedSessions.length > 0 ? "attention" : undefined}
            />
            <Metric
              label={t("runtime.metrics.unmeasured.label")}
              value={formatNumber(unmeasuredSessions.length)}
              detail={t("runtime.metrics.unmeasured.detail")}
              tone={unmeasuredSessions.length > 0 ? "attention" : undefined}
            />
            <Metric
              label={t("runtime.metrics.telemetry.label")}
              value={observability?.enabled ? "ENABLED" : "DISABLED"}
              detail={t("runtime.metrics.telemetry.detail", {
                cursor: formatNumber(observability?.cursor ?? 0),
                pending: formatNumber(observability?.pending ?? 0),
              })}
              tone={observability?.enabled ? "success" : undefined}
            />
          </section>

          <div className="overview-grid">
            <Panel className="overview-panel">
              <div className="section-heading">
                <h2>{t("runtime.availability.title")}</h2>
                <span className="muted">{t("runtime.availability.hint")}</span>
              </div>
              <dl className="overview-facts">
                <div>
                  <dt>{t("runtime.availability.core")}</dt>
                  <dd>
                    <StatusBadge
                      status={health?.healthy ? "HEALTHY" : "UNHEALTHY"}
                    />
                  </dd>
                </div>
                <div>
                  <dt>{t("runtime.availability.workers")}</dt>
                  <dd>{formatNumber(workers.length)}</dd>
                </div>
                <div>
                  <dt>{t("runtime.availability.sessions")}</dt>
                  <dd>{formatNumber(sessions.length)}</dd>
                </div>
                <div>
                  <dt>{t("runtime.availability.modelCalls")}</dt>
                  <dd>
                    {t("runtime.availability.sampled", {
                      count: formatNumber(modelCalls.length),
                    })}
                  </dd>
                </div>
                <div>
                  <dt>{t("runtime.availability.openTurns")}</dt>
                  <dd>{formatNumber(turnBudget?.reserved_turns ?? 0)}</dd>
                </div>
              </dl>
              <div className="ui-callout ui-callout--info">
                {t("runtime.availability.boundaryNote")}
              </div>
            </Panel>

            <Panel className="overview-panel">
              <div className="section-heading">
                <h2>{t("runtime.states.title")}</h2>
                <Link to="/runtime?tab=sessions">
                  {t("runtime.states.inspect")}
                </Link>
              </div>
              {sessions.length === 0 ? (
                <EmptyState
                  title={t("runtime.states.emptyTitle")}
                  description={t("runtime.states.emptyDescription")}
                />
              ) : (
                <div className="overview-badges">
                  {Object.entries(sessionCounts(sessions)).map(
                    ([state, count]) => (
                      <StatusBadge
                        key={state}
                        status={state}
                        label={`${state}: ${count}`}
                      />
                    ),
                  )}
                </div>
              )}
            </Panel>
          </div>

          <div className="overview-grid">
            <Panel className="overview-panel">
              <div className="section-heading">
                <h2>{t("runtime.workers.title")}</h2>
                <span className="muted">{t("runtime.workers.hint")}</span>
              </div>
              {workers.length === 0 ? (
                <EmptyState
                  title={t("runtime.workers.emptyTitle")}
                  description={t("runtime.workers.emptyDescription")}
                />
              ) : (
                <section
                  className="table-responsive"
                  aria-label={t("runtime.workers.table.label")}
                >
                  <table>
                    <thead>
                      <tr>
                        <th>{t("runtime.workers.table.worker")}</th>
                        <th>{t("runtime.workers.table.lastRegistration")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {workers.map((worker) => (
                        <tr key={worker.worker_id}>
                          <td data-label={t("runtime.workers.table.worker")}>
                            <code>{worker.worker_id}</code>
                          </td>
                          <td
                            className="muted"
                            data-label={t(
                              "runtime.workers.table.lastRegistration",
                            )}
                          >
                            {formatDate(worker.updated_at)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </section>
              )}
            </Panel>

            <Panel className="overview-panel">
              <div className="section-heading">
                <h2>{t("runtime.unresolved.title")}</h2>
                <Link to="/runtime?tab=model-calls">
                  {t("runtime.unresolved.open")}
                </Link>
              </div>
              {unresolvedCalls.length === 0 ? (
                <EmptyState
                  title={t("runtime.unresolved.emptyTitle")}
                  description={t("runtime.unresolved.emptyDescription")}
                />
              ) : (
                <ul className="overview-decisions">
                  {unresolvedCalls.slice(0, 4).map((call) => (
                    <li key={call.reservation_id}>
                      <span>
                        <code>{shortId(call.reservation_id)}</code> ·{" "}
                        {call.role} · {call.provider}/{call.model}
                      </span>
                      <StatusBadge status={call.state} />
                      <p className="muted">
                        {t("runtime.unresolved.ceiling", {
                          cost: formatMicros(call.max_cost_micros),
                        })}{" "}
                        · {formatDate(call.updated_at)}
                      </p>
                    </li>
                  ))}
                </ul>
              )}
            </Panel>
          </div>
        </div>
      )}

      {tab === "sessions" && (
        <div
          id="panel-sessions"
          role="tabpanel"
          aria-labelledby="tab-sessions"
          className="campaign-tab-panel"
        >
          <Panel>
            <div className="section-heading">
              <h2>
                {t("runtime.sessions.title", {
                  count: formatNumber(filteredSessions.length),
                })}
              </h2>
              <span className="muted">{t("runtime.sessions.hint")}</span>
            </div>
            <div className="filters">
              <Select
                label={t("runtime.filters.state")}
                value={sessionState}
                onChange={(event) => setSessionState(event.target.value)}
              >
                {sessionStates.map((state) => (
                  <option key={state} value={state}>
                    {state === "all" ? t("runtime.filters.allStates") : state}
                  </option>
                ))}
              </Select>
              <Select
                label={t("runtime.filters.usage")}
                value={sessionUsage}
                onChange={(event) => setSessionUsage(event.target.value)}
              >
                <option value="all">{t("runtime.filters.allUsage")}</option>
                <option value="measured">
                  {t("runtime.filters.measured")}
                </option>
                <option value="unmeasured">
                  {t("runtime.filters.unmeasured")}
                </option>
              </Select>
              <Select
                label={t("runtime.filters.cleanup")}
                value={sessionCleanup}
                onChange={(event) => setSessionCleanup(event.target.value)}
              >
                <option value="all">{t("runtime.filters.allCleanup")}</option>
                <option value="recorded">
                  {t("runtime.filters.receiptRecorded")}
                </option>
                <option value="missing">
                  {t("runtime.filters.noReceipt")}
                </option>
              </Select>
              <Input
                label={t("runtime.filters.workerOrRoute")}
                value={sessionWorker}
                onChange={(event) => setSessionWorker(event.target.value)}
                placeholder={t("runtime.filters.workerOrRoutePlaceholder")}
              />
            </div>
            <ParticipantSessionList
              sessions={filteredSessions}
              emptyTitle={t("runtime.sessions.emptyTitle")}
              emptyDescription={t("runtime.sessions.emptyDescription")}
            />
          </Panel>
        </div>
      )}

      {tab === "model-calls" && (
        <div
          id="panel-model-calls"
          role="tabpanel"
          aria-labelledby="tab-model-calls"
          className="campaign-tab-panel"
        >
          <Panel>
            <div className="section-heading">
              <h2>
                {t("runtime.calls.title", {
                  count: formatNumber(filteredCalls.length),
                })}
              </h2>
              <span className="muted">{t("runtime.calls.hint")}</span>
            </div>
            <div className="filters">
              <Select
                label={t("runtime.filters.role")}
                value={callRole}
                onChange={(event) => setCallRole(event.target.value)}
              >
                <option value="all">{t("runtime.filters.allRoles")}</option>
                <option value="lead">{t("runtime.filters.roleLead")}</option>
                <option value="semantic_reviewer">
                  {t("runtime.filters.roleSemanticReviewer")}
                </option>
                <option value="evaluation">
                  {t("runtime.filters.roleEvaluation")}
                </option>
              </Select>
              <Select
                label={t("runtime.filters.state")}
                value={callState}
                onChange={(event) => setCallState(event.target.value)}
              >
                {callStates.map((state) => (
                  <option key={state} value={state}>
                    {state === "all" ? t("runtime.filters.allStates") : state}
                  </option>
                ))}
              </Select>
              <Select
                label={t("runtime.filters.usage")}
                value={callUsage}
                onChange={(event) => setCallUsage(event.target.value)}
              >
                <option value="all">{t("runtime.filters.allUsage")}</option>
                <option value="measured">
                  {t("runtime.filters.measured")}
                </option>
                <option value="unmeasured">
                  {t("runtime.filters.unmeasured")}
                </option>
              </Select>
            </div>
            {filteredCalls.length === 0 ? (
              <EmptyState
                title={t("runtime.calls.emptyTitle")}
                description={t("runtime.calls.emptyDescription")}
              />
            ) : (
              <section
                className="table-responsive"
                aria-label={t("runtime.calls.table.label")}
              >
                <table>
                  <thead>
                    <tr>
                      <th>{t("runtime.calls.table.reservation")}</th>
                      <th>{t("runtime.calls.table.roleScope")}</th>
                      <th>{t("runtime.calls.table.route")}</th>
                      <th>{t("runtime.calls.table.state")}</th>
                      <th>{t("runtime.calls.table.usage")}</th>
                      <th>{t("runtime.calls.table.ceiling")}</th>
                      <th>{t("runtime.calls.table.updated")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {filteredCalls.map((call) => (
                      <tr key={call.reservation_id}>
                        <td data-label={t("runtime.calls.table.reservation")}>
                          <code>{shortId(call.reservation_id)}</code>
                        </td>
                        <td data-label={t("runtime.calls.table.roleScope")}>
                          {call.role}
                          <div className="muted">
                            <code>{shortId(call.scope_id)}</code> ·{" "}
                            {call.purpose}
                          </div>
                        </td>
                        <td data-label={t("runtime.calls.table.route")}>
                          {call.provider}/{call.model}
                        </td>
                        <td data-label={t("runtime.calls.table.state")}>
                          <StatusBadge status={call.state} />
                          {call.error && (
                            <div className="error">{call.error}</div>
                          )}
                        </td>
                        <td data-label={t("runtime.calls.table.usage")}>
                          {call.usage_known
                            ? t("runtime.calls.tokens", {
                                tokens: formatNumber(call.total_tokens),
                                cost: formatMicros(call.cost_micros),
                              })
                            : "UNMEASURED"}
                        </td>
                        <td data-label={t("runtime.calls.table.ceiling")}>
                          {formatMicros(call.max_cost_micros)}
                        </td>
                        <td
                          className="muted"
                          data-label={t("runtime.calls.table.updated")}
                        >
                          {formatDate(call.updated_at)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>
            )}
          </Panel>

          {unresolvedCalls.length > 0 && (
            <Panel>
              <div className="section-heading">
                <h2>
                  {t("runtime.calls.unresolvedTitle", {
                    count: formatNumber(unresolvedCalls.length),
                  })}
                </h2>
                <span className="muted">
                  {t("runtime.calls.unresolvedHint")}
                </span>
              </div>
              <div className="decision-list">
                {unresolvedCalls.map((call) => (
                  <div className="decision-item" key={call.reservation_id}>
                    <div className="decision-item-head">
                      <StatusBadge status={call.state} />
                      <h3>
                        <code>{shortId(call.reservation_id)}</code> ·{" "}
                        {call.role}
                      </h3>
                      <span className="muted">
                        {t("runtime.unresolved.ceiling", {
                          cost: formatMicros(call.max_cost_micros),
                        })}{" "}
                        · {formatDate(call.updated_at)}
                      </span>
                    </div>
                    <p>
                      {call.provider}/{call.model} · {t("runtime.calls.scope")}{" "}
                      <code>{shortId(call.scope_id)}</code> · {call.purpose}
                    </p>
                    {call.error && <p className="error">{call.error}</p>}
                    <ModelCallResolution
                      modelCallId={call.reservation_id}
                      onResolved={load}
                      compact
                    />
                  </div>
                ))}
              </div>
            </Panel>
          )}
        </div>
      )}

      {tab === "diagnostics" && (
        <div
          id="panel-diagnostics"
          role="tabpanel"
          aria-labelledby="tab-diagnostics"
          className="campaign-tab-panel"
        >
          <Panel>
            <div className="section-heading">
              <h2>{t("runtime.telemetry.title")}</h2>
              <StatusBadge
                status={observability?.enabled ? "ENABLED" : "DISABLED"}
              />
            </div>
            {observability === null ? (
              <EmptyState
                title={t("runtime.telemetry.emptyTitle")}
                description={t("runtime.telemetry.emptyDescription")}
              />
            ) : (
              <>
                <dl className="detail">
                  <dt>{t("runtime.telemetry.cursor")}</dt>
                  <dd>{formatNumber(observability.cursor)}</dd>
                  <dt>{t("runtime.telemetry.exported")}</dt>
                  <dd>{formatNumber(observability.exported)}</dd>
                  <dt>{t("runtime.telemetry.pending")}</dt>
                  <dd>{formatNumber(observability.pending)}</dd>
                  <dt>{t("runtime.telemetry.droppedHints")}</dt>
                  <dd>{formatNumber(observability.dropped)}</dd>
                  <dt>{t("runtime.telemetry.failedAttempts")}</dt>
                  <dd>{formatNumber(observability.failed_attempts)}</dd>
                </dl>
                {observability.last_error && (
                  <p className="error">{observability.last_error}</p>
                )}
                <p className="muted">{t("runtime.telemetry.note")}</p>
              </>
            )}
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>{t("runtime.policy.title")}</h2>
              <Link to="/settings">{t("runtime.policy.openSettings")}</Link>
            </div>
            {settings ? (
              <dl className="detail">
                <dt>{t("runtime.policy.semanticReview")}</dt>
                <dd>
                  <StatusBadge
                    status={
                      settings.semanticReviewEnabled ? "ENABLED" : "DISABLED"
                    }
                  />{" "}
                  {settings.semanticReviewEnabled
                    ? t("runtime.policy.modelReceipts")
                    : t("runtime.policy.deterministicReceipts")}
                </dd>
                <dt>{t("runtime.policy.maxOutputTokens")}</dt>
                <dd>{formatNumber(settings.semanticReviewMaxOutputTokens)}</dd>
                <dt>{t("runtime.policy.updated")}</dt>
                <dd>{formatDate(settings.updatedAt)}</dd>
              </dl>
            ) : (
              <p className="muted">{t("runtime.policy.empty")}</p>
            )}
            <p className="muted">{t("runtime.policy.note")}</p>
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>{t("runtime.boundaries.title")}</h2>
              <span className="muted">{t("runtime.boundaries.hint")}</span>
            </div>
            <dl className="detail">
              <dt>{t("runtime.boundaries.core")}</dt>
              <dd>
                <StatusBadge
                  status={health?.healthy ? "HEALTHY" : "UNHEALTHY"}
                />{" "}
                {t("runtime.boundaries.coreVersion", {
                  version: health?.version ?? t("common.unknown"),
                })}
              </dd>
              <dt>{t("runtime.boundaries.workers")}</dt>
              <dd>
                {t("runtime.boundaries.workersDetail", {
                  count: formatNumber(workers.length),
                })}
              </dd>
              <dt>{t("runtime.boundaries.supervisor")}</dt>
              <dd>{t("runtime.boundaries.notExposed")}</dd>
              <dt>{t("runtime.boundaries.nestedRuntime")}</dt>
              <dd>{t("runtime.boundaries.notExposed")}</dd>
              <dt>{t("runtime.boundaries.broker")}</dt>
              <dd>{t("runtime.boundaries.notExposed")}</dd>
            </dl>
            <p className="muted">{t("runtime.boundaries.note")}</p>
          </Panel>
        </div>
      )}
    </div>
  );
}
