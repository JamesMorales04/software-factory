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
  type WorkerSummary,
} from "../coreApi";
import { formatCostMicros, formatTimestamp, shortId } from "../format";

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
    ] = await Promise.all([
      coreApi.health(),
      coreApi.listWorkers(),
      coreApi.listProviderSessions(),
      coreApi.listModelCalls(100),
      coreApi.listUnresolvedModelCalls(),
      coreApi.getObservability(),
      coreApi.getRuntimeSettings(),
    ]);
    setHealth(nextHealth);
    setWorkers(nextWorkers);
    setSessions(nextSessions);
    setModelCalls(recentCalls);
    setUnresolvedCalls(unresolved);
    setObservability(telemetry);
    setSettings(runtimeSettings);
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
              : "Failed to load runtime state.",
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
  }, [load]);

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
  const sessionUnmeasuredBound = unmeasuredSessions.reduce(
    (sum, session) => sum + session.cost_micros,
    0,
  );
  const reservationCeiling = unresolvedCalls.reduce(
    (sum, call) => sum + call.max_cost_micros,
    0,
  );
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
        message={error}
        action={<Button onClick={() => window.location.reload()}>Retry</Button>}
      />
    );
  }
  if (!loaded) return <LoadingState label="Loading runtime state…" />;

  const tabs: TabItem[] = [
    { id: "overview", label: "Overview" },
    {
      id: "sessions",
      label: "Sessions",
      count: sessions.length,
    },
    {
      id: "model-calls",
      label: "Model calls",
      count: allCalls.length,
    },
    { id: "diagnostics", label: "Diagnostics" },
  ];

  return (
    <div className="runtime-workspace">
      <PageHeader
        title="Runtime"
        description="Durable Core projections for provider sessions, model-call reservations, cleanup and telemetry. A missing record is not proof of successful cleanup."
        meta={
          <>
            <StatusBadge
              status={health?.healthy ? "HEALTHY" : "UNHEALTHY"}
              label={health?.healthy ? "Core healthy" : "Core unavailable"}
            />
            {health && <span>Core {health.version}</span>}
            <span>
              Fetched {lastLoadedAt ? formatTimestamp(lastLoadedAt) : "—"}
            </span>
            <span>{workers.length} registered Worker(s)</span>
          </>
        }
        actions={
          <>
            <Link className={buttonClass("secondary", "sm")} to="/inbox">
              Decision Inbox
            </Link>
            <Link className={buttonClass("quiet", "sm")} to="/settings">
              Settings
            </Link>
          </>
        }
      />
      {error && (
        <div className="ui-callout ui-callout--danger" role="alert">
          {error} The displayed runtime snapshot may be stale.
        </div>
      )}

      <Tabs
        items={tabs}
        value={tab}
        onChange={selectTab}
        label="Runtime sections"
      />

      {tab === "overview" && (
        <div
          id="panel-overview"
          role="tabpanel"
          aria-labelledby="tab-overview"
          className="campaign-tab-panel"
        >
          <section className="overview-metrics" aria-label="Runtime summary">
            <Metric
              label="Measured spend"
              value={formatCostMicros(measuredCost)}
              detail={`${measuredSessions.length} measured session(s), ${measuredCalls.length} measured call(s)`}
            />
            <Metric
              label="Reserved ceiling"
              value={formatCostMicros(
                reservationCeiling + sessionUnmeasuredBound,
              )}
              detail="Unresolved calls use max cost; unmeasured session cost is a bound"
              tone="attention"
            />
            <Metric
              label="Unresolved calls"
              value={unresolvedCalls.length}
              detail="AMBIGUOUS or UNKNOWN Core reservations"
              tone={unresolvedCalls.length > 0 ? "attention" : undefined}
            />
            <Metric
              label="Unresolved sessions"
              value={unresolvedSessions.length}
              detail={`${sessions.length} session(s) recorded`}
              tone={unresolvedSessions.length > 0 ? "attention" : undefined}
            />
            <Metric
              label="Unmeasured sessions"
              value={unmeasuredSessions.length}
              detail="Excluded from measured spend"
              tone={unmeasuredSessions.length > 0 ? "attention" : undefined}
            />
            <Metric
              label="Telemetry"
              value={observability?.enabled ? "ENABLED" : "DISABLED"}
              detail={`cursor ${observability?.cursor ?? 0} · ${observability?.pending ?? 0} pending`}
              tone={observability?.enabled ? "success" : undefined}
            />
          </section>

          <div className="overview-grid">
            <Panel className="overview-panel">
              <div className="section-heading">
                <h2>Service availability</h2>
                <span className="muted">Only Core-backed facts are shown</span>
              </div>
              <dl className="overview-facts">
                <div>
                  <dt>Core</dt>
                  <dd>
                    <StatusBadge
                      status={health?.healthy ? "HEALTHY" : "UNHEALTHY"}
                    />
                  </dd>
                </div>
                <div>
                  <dt>Registered Workers</dt>
                  <dd>{workers.length}</dd>
                </div>
                <div>
                  <dt>Provider sessions</dt>
                  <dd>{sessions.length}</dd>
                </div>
                <div>
                  <dt>Model calls</dt>
                  <dd>{allCalls.length}</dd>
                </div>
              </dl>
              <div className="ui-callout ui-callout--info">
                Supervisor and nested runtime state are not exposed by the
                current Core API. Office does not infer their health from a
                missing record, a disconnected view or Compose process state.
              </div>
            </Panel>

            <Panel className="overview-panel">
              <div className="section-heading">
                <h2>Session states</h2>
                <Link to="/runtime?tab=sessions">Inspect sessions</Link>
              </div>
              {sessions.length === 0 ? (
                <EmptyState
                  title="No provider sessions"
                  description="No provider session is recorded by Core."
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
                <h2>Registered Workers</h2>
                <span className="muted">
                  Registration is not a heartbeat or health certificate
                </span>
              </div>
              {workers.length === 0 ? (
                <EmptyState
                  title="No registered Workers"
                  description="A Worker appears after registration; this does not prove it is online."
                />
              ) : (
                <table>
                  <thead>
                    <tr>
                      <th>Worker</th>
                      <th>Last registration record</th>
                    </tr>
                  </thead>
                  <tbody>
                    {workers.map((worker) => (
                      <tr key={worker.worker_id}>
                        <td>
                          <code>{worker.worker_id}</code>
                        </td>
                        <td className="muted">
                          {formatTimestamp(worker.updated_at)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </Panel>

            <Panel className="overview-panel">
              <div className="section-heading">
                <h2>Unresolved reservations</h2>
                <Link to="/runtime?tab=model-calls">Open model calls</Link>
              </div>
              {unresolvedCalls.length === 0 ? (
                <EmptyState
                  title="No unresolved model calls"
                  description="No AMBIGUOUS or UNKNOWN Core reservation is recorded."
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
                        Ceiling {formatCostMicros(call.max_cost_micros)} ·{" "}
                        {formatTimestamp(call.updated_at)}
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
              <h2>Provider sessions ({filteredSessions.length} shown)</h2>
              <span className="muted">
                Cleanup receipts are recorded evidence, not isolation proof
              </span>
            </div>
            <div className="filters">
              <Select
                label="State"
                value={sessionState}
                onChange={(event) => setSessionState(event.target.value)}
              >
                {sessionStates.map((state) => (
                  <option key={state} value={state}>
                    {state === "all" ? "All states" : state}
                  </option>
                ))}
              </Select>
              <Select
                label="Usage"
                value={sessionUsage}
                onChange={(event) => setSessionUsage(event.target.value)}
              >
                <option value="all">All usage</option>
                <option value="measured">Measured</option>
                <option value="unmeasured">Unmeasured</option>
              </Select>
              <Select
                label="Cleanup"
                value={sessionCleanup}
                onChange={(event) => setSessionCleanup(event.target.value)}
              >
                <option value="all">All cleanup states</option>
                <option value="recorded">Receipt recorded</option>
                <option value="missing">No receipt</option>
              </Select>
              <Input
                label="Worker or route"
                value={sessionWorker}
                onChange={(event) => setSessionWorker(event.target.value)}
                placeholder="Filter by worker, provider or model"
              />
            </div>
            <ParticipantSessionList
              sessions={filteredSessions}
              emptyTitle="No matching sessions"
              emptyDescription="Adjust the filters; no state is inferred for hidden records."
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
              <h2>Core-owned model calls ({filteredCalls.length} shown)</h2>
              <span className="muted">
                Recent calls are sampled to 100; unresolved calls use the full
                ledger
              </span>
            </div>
            <div className="filters">
              <Select
                label="Role"
                value={callRole}
                onChange={(event) => setCallRole(event.target.value)}
              >
                <option value="all">All roles</option>
                <option value="lead">Lead</option>
                <option value="semantic_reviewer">Semantic reviewer</option>
                <option value="evaluation">Evaluation</option>
              </Select>
              <Select
                label="State"
                value={callState}
                onChange={(event) => setCallState(event.target.value)}
              >
                {callStates.map((state) => (
                  <option key={state} value={state}>
                    {state === "all" ? "All states" : state}
                  </option>
                ))}
              </Select>
              <Select
                label="Usage"
                value={callUsage}
                onChange={(event) => setCallUsage(event.target.value)}
              >
                <option value="all">All usage</option>
                <option value="measured">Measured</option>
                <option value="unmeasured">Unmeasured</option>
              </Select>
            </div>
            {filteredCalls.length === 0 ? (
              <EmptyState
                title="No matching model calls"
                description="Adjust the filters; unresolved reservations are never hidden by the recent-call sample."
              />
            ) : (
              <table>
                <thead>
                  <tr>
                    <th>Reservation</th>
                    <th>Role / scope</th>
                    <th>Route</th>
                    <th>State</th>
                    <th>Usage</th>
                    <th>Ceiling</th>
                    <th>Updated</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredCalls.map((call) => (
                    <tr key={call.reservation_id}>
                      <td>
                        <code>{shortId(call.reservation_id)}</code>
                      </td>
                      <td>
                        {call.role}
                        <div className="muted">
                          <code>{shortId(call.scope_id)}</code> · {call.purpose}
                        </div>
                      </td>
                      <td>
                        {call.provider}/{call.model}
                      </td>
                      <td>
                        <StatusBadge status={call.state} />
                        {call.error && (
                          <div className="error">{call.error}</div>
                        )}
                      </td>
                      <td>
                        {call.usage_known
                          ? `${call.total_tokens.toLocaleString()} tokens / ${formatCostMicros(call.cost_micros)}`
                          : "UNMEASURED"}
                      </td>
                      <td>{formatCostMicros(call.max_cost_micros)}</td>
                      <td className="muted">
                        {formatTimestamp(call.updated_at)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </Panel>

          {unresolvedCalls.length > 0 && (
            <Panel>
              <div className="section-heading">
                <h2>Unresolved reservations ({unresolvedCalls.length})</h2>
                <span className="muted">
                  Releasing is explicit and audited by Core; it is not automatic
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
                        ceiling {formatCostMicros(call.max_cost_micros)} ·{" "}
                        {formatTimestamp(call.updated_at)}
                      </span>
                    </div>
                    <p>
                      {call.provider}/{call.model} · scope{" "}
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
              <h2>Telemetry export</h2>
              <StatusBadge
                status={observability?.enabled ? "ENABLED" : "DISABLED"}
              />
            </div>
            {observability === null ? (
              <EmptyState
                title="No observability status"
                description="Core did not return telemetry status."
              />
            ) : (
              <>
                <dl className="detail">
                  <dt>Cursor</dt>
                  <dd>{observability.cursor}</dd>
                  <dt>Exported</dt>
                  <dd>{observability.exported}</dd>
                  <dt>Pending</dt>
                  <dd>{observability.pending}</dd>
                  <dt>Dropped hints</dt>
                  <dd>{observability.dropped}</dd>
                  <dt>Failed attempts</dt>
                  <dd>{observability.failed_attempts}</dd>
                </dl>
                {observability.last_error && (
                  <p className="error">{observability.last_error}</p>
                )}
                <p className="muted">
                  Exporter failure is telemetry degradation, not authoritative
                  Factory state.
                </p>
              </>
            )}
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>Runtime policy</h2>
              <Link to="/settings">Open Settings</Link>
            </div>
            {settings ? (
              <dl className="detail">
                <dt>Semantic review</dt>
                <dd>
                  <StatusBadge
                    status={
                      settings.semanticReviewEnabled ? "ENABLED" : "DISABLED"
                    }
                  />{" "}
                  {settings.semanticReviewEnabled
                    ? "MODEL receipts authorized"
                    : "DETERMINISTIC receipts only"}
                </dd>
                <dt>Max output tokens</dt>
                <dd>
                  {settings.semanticReviewMaxOutputTokens.toLocaleString()}
                </dd>
                <dt>Updated</dt>
                <dd>{formatTimestamp(settings.updatedAt)}</dd>
              </dl>
            ) : (
              <p className="muted">No runtime settings returned.</p>
            )}
            <p className="muted">
              Provider credentials are never rendered here. Configure them in
              Settings.
            </p>
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>Service boundaries</h2>
              <span className="muted">
                Not all runtime components expose state
              </span>
            </div>
            <dl className="detail">
              <dt>Core</dt>
              <dd>
                <StatusBadge
                  status={health?.healthy ? "HEALTHY" : "UNHEALTHY"}
                />{" "}
                version {health?.version ?? "not returned"}
              </dd>
              <dt>Workers</dt>
              <dd>
                {workers.length} registered; registration record only, not a
                heartbeat
              </dd>
              <dt>Supervisor</dt>
              <dd>Not exposed by the current Core API</dd>
              <dt>Nested Podman runtime</dt>
              <dd>Not exposed by the current Core API</dd>
              <dt>Broker</dt>
              <dd>Not exposed by the current Core API</dd>
            </dl>
            <p className="muted">
              Compose logs remain the diagnostic source for components without a
              Core projection. Office does not infer stopped, healthy or cleaned
              from a missing browser record.
            </p>
          </Panel>
        </div>
      )}
    </div>
  );
}
