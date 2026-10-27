import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Badge } from "../components/ui/Badge";
import { Button, buttonClass } from "../components/ui/Button";
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
  type DecisionItem,
  type OfficeOverview,
} from "../coreApi";
import { formatCostMicros, formatTimestamp, shortId } from "../format";

function toneForGate(outcome: string) {
  if (outcome === "ACCEPTED") return "success" as const;
  if (outcome === "REPAIR_REQUIRED") return "attention" as const;
  if (outcome === "NOT_TESTED") return "muted" as const;
  return "danger" as const;
}

function toneForDecision(severity: DecisionItem["severity"]) {
  if (severity === "critical") return "danger" as const;
  if (severity === "attention") return "attention" as const;
  return "info" as const;
}

function Metric({
  label,
  value,
  detail,
  to,
}: {
  label: string;
  value: number | string;
  detail?: string;
  to?: string;
}) {
  return (
    <Panel className="overview-metric">
      <p className="overview-metric-label">{label}</p>
      <p className="overview-metric-value">{value}</p>
      {detail && <p className="overview-metric-detail">{detail}</p>}
      {to && (
        <Link
          className="overview-metric-link"
          to={to}
          aria-label={`Open ${label}`}
        >
          Open
        </Link>
      )}
    </Panel>
  );
}

export function Overview() {
  const [data, setData] = useState<OfficeOverview | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const next = await coreApi.getOverview();
        if (!active) return;
        setData(next);
        setError(null);
      } catch (cause) {
        if (!active) return;
        setError(
          cause instanceof CoreApiError
            ? cause.message
            : "Failed to load the Core overview.",
        );
      }
    }
    void load();
    const timer = setInterval(() => void load(), 5000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  if (error && !data) {
    return (
      <ErrorState
        message={error}
        action={<Button onClick={() => window.location.reload()}>Retry</Button>}
      />
    );
  }
  if (!data) {
    return <LoadingState label="Loading Core overview…" />;
  }

  const gateEntries = Object.entries(data.gate_outcomes);
  const accepted = data.gate_outcomes.ACCEPTED ?? 0;
  const measuredCost =
    data.budget.measured_session_cost_micros +
    data.budget.measured_model_cost_micros;
  const measuredTokens =
    data.budget.measured_session_tokens + data.budget.measured_model_tokens;

  return (
    <div>
      <PageHeader
        title="Overview"
        description="Operational state from durable Core records. Unknown and unmeasured values are shown as such."
        meta={
          <>
            <span>Core snapshot {formatTimestamp(data.as_of)}</span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>{data.runtime.workers} registered Workers</span>
          </>
        }
        actions={
          <>
            <Link className={buttonClass("secondary")} to="/agents">
              Open Agent Office
            </Link>
            <Link className={buttonClass("quiet")} to="/inbox">
              Open Decision Inbox
            </Link>
            <Link className={buttonClass("quiet")} to="/runtime">
              Runtime
            </Link>
          </>
        }
      />

      {error && <p className="error">{error}</p>}

      <section className="overview-metrics" aria-label="Operational summary">
        <Metric
          label="Authorized campaigns"
          value={data.counts.authorized_campaigns}
          detail={`${data.counts.campaigns_awaiting_authorization} awaiting authorization`}
          to="/campaigns"
        />
        <Metric
          label="Running agents"
          value={data.counts.active_agents}
          detail={`${data.counts.unresolved_sessions} unresolved session${data.counts.unresolved_sessions === 1 ? "" : "s"}`}
          to="/agents"
        />
        <Metric
          label="Pending decisions"
          value={data.counts.pending_decisions}
          detail="Actionable Core-backed items"
          to="/inbox"
        />
        <Metric
          label="Blocked signals"
          value={data.counts.blocked_signals}
          detail="Actions, Runs, Campaigns or Gates"
          to="/campaigns"
        />
        <Metric
          label="Accepted candidates"
          value={accepted}
          detail={`${data.counts.candidates} recorded`}
          to="/campaigns"
        />
        <Metric
          label="Measured spend"
          value={formatCostMicros(measuredCost)}
          detail={`${measuredTokens.toLocaleString()} measured tokens`}
          to="/runtime"
        />
      </section>

      <div className="overview-grid">
        <Panel className="overview-panel">
          <div className="section-heading">
            <h2>Quality Gate outcomes</h2>
            <Link to="/campaigns">All candidates</Link>
          </div>
          {gateEntries.length === 0 ? (
            <EmptyState
              title="No Gate decisions"
              description="A candidate appears here after independent verifier and reviewer evidence is recorded."
            />
          ) : (
            <div className="overview-badges">
              {gateEntries.map(([outcome, count]) => (
                <Badge key={outcome} tone={toneForGate(outcome)}>
                  {outcome}: {count}
                </Badge>
              ))}
            </div>
          )}
          <dl className="overview-facts">
            <div>
              <dt>Unmeasured sessions</dt>
              <dd>{data.budget.unmeasured_sessions}</dd>
            </div>
            <div>
              <dt>Unmeasured model calls</dt>
              <dd>{data.budget.unmeasured_model_calls}</dd>
            </div>
            <div>
              <dt>Unmeasured bound</dt>
              <dd>
                {formatCostMicros(data.budget.unmeasured_bound_cost_micros)}
              </dd>
            </div>
            <div>
              <dt>Core</dt>
              <dd>
                <Badge tone={data.runtime.core_healthy ? "success" : "danger"}>
                  {data.runtime.core_healthy ? "HEALTHY" : "UNHEALTHY"}
                </Badge>
              </dd>
            </div>
          </dl>
          <p className="muted overview-footnote">{data.budget.note}</p>
        </Panel>

        <Panel className="overview-panel">
          <div className="section-heading">
            <h2>Decision Inbox</h2>
            <Link to="/inbox">Open Inbox</Link>
          </div>
          {data.recent_decisions.length === 0 ? (
            <EmptyState
              title="No decisions waiting"
              description="New approvals or interventions appear here when Core records them."
            />
          ) : (
            <ul className="overview-decisions">
              {data.recent_decisions.map((item) => (
                <li key={item.id}>
                  <Link to={item.target}>{item.title}</Link>
                  <Badge tone={toneForDecision(item.severity)}>
                    {item.severity}
                  </Badge>
                  <p className="muted">{item.detail}</p>
                </li>
              ))}
            </ul>
          )}
        </Panel>
      </div>

      <div className="overview-grid">
        <Panel className="overview-panel">
          <div className="section-heading">
            <h2>Recent accepted candidates</h2>
            <Link to="/campaigns">Campaigns</Link>
          </div>
          {data.recent_accepted.length === 0 ? (
            <EmptyState
              title="No accepted candidates"
              description="Acceptance is shown only when Core records an ACCEPTED Quality Gate."
            />
          ) : (
            <ul className="overview-accepted">
              {data.recent_accepted.map((candidate) => (
                <li key={candidate.candidate_id}>
                  <Badge tone="success">ACCEPTED</Badge>
                  <Link
                    to={
                      candidate.campaign_id
                        ? `/campaigns/${candidate.campaign_id}`
                        : "/campaigns"
                    }
                  >
                    {shortId(candidate.candidate_id)}
                  </Link>
                  <code>{shortId(candidate.commit_sha, 12)}</code>
                  <span className="muted">
                    {formatTimestamp(candidate.decided_at)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Panel>

        <Panel className="overview-panel">
          <div className="section-heading">
            <h2>Runtime health</h2>
            <Link to="/runtime">Inspect runtime</Link>
          </div>
          <dl className="overview-facts">
            <div>
              <dt>Active agents</dt>
              <dd>{data.runtime.active_agents}</dd>
            </div>
            <div>
              <dt>Unresolved sessions</dt>
              <dd>{data.runtime.unresolved_sessions}</dd>
            </div>
            <div>
              <dt>Unresolved model calls</dt>
              <dd>{data.runtime.unresolved_model_calls}</dd>
            </div>
            <div>
              <dt>Telemetry export</dt>
              <dd>
                <Badge
                  tone={
                    data.runtime.observability.enabled ? "success" : "muted"
                  }
                >
                  {data.runtime.observability.enabled ? "ENABLED" : "DISABLED"}
                </Badge>
              </dd>
            </div>
          </dl>
        </Panel>
      </div>

      <Panel className="overview-panel">
        <div className="section-heading">
          <h2>Recent events</h2>
          <span className="muted">{data.recent_events.length} shown</span>
        </div>
        {data.recent_events.length === 0 ? (
          <EmptyState
            title="No events"
            description="Core records an event when Project, Action, Run or quality state changes."
          />
        ) : (
          <ul className="events">
            {data.recent_events.map((event) => (
              <li key={event.sequence}>
                <div className="event-head">
                  <code>#{event.sequence}</code>
                  <strong>{event.kind}</strong>
                  <span className="muted">
                    {shortId(event.subject_id)} ·{" "}
                    {formatTimestamp(event.recorded_at)}
                  </span>
                </div>
                <p className="event-detail">{event.detail}</p>
              </li>
            ))}
          </ul>
        )}
      </Panel>
    </div>
  );
}
