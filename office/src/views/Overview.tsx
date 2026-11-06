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
import { useI18n } from "../i18n";

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
  openLabel,
  openAria,
}: {
  label: string;
  value: number | string;
  detail?: string;
  to?: string;
  openLabel: string;
  openAria: string;
}) {
  return (
    <Panel className="overview-metric">
      <p className="overview-metric-label">{label}</p>
      <p className="overview-metric-value">{value}</p>
      {detail && <p className="overview-metric-detail">{detail}</p>}
      {to && (
        <Link className="overview-metric-link" to={to} aria-label={openAria}>
          {openLabel}
        </Link>
      )}
    </Panel>
  );
}

export function Overview() {
  const { t, formatNumber } = useI18n();
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
            : t("overview.error.load"),
        );
      }
    }
    void load();
    const timer = setInterval(() => void load(), 5000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [t]);

  if (error && !data) {
    return (
      <ErrorState
        message={error}
        action={
          <Button onClick={() => window.location.reload()}>
            {t("common.retry")}
          </Button>
        }
      />
    );
  }
  if (!data) {
    return <LoadingState label={t("overview.loading")} />;
  }

  const gateEntries = Object.entries(data.gate_outcomes);
  const accepted = data.gate_outcomes.ACCEPTED ?? 0;
  const measuredCost =
    data.budget.measured_session_cost_micros +
    data.budget.measured_model_cost_micros;
  const measuredTokens =
    data.budget.measured_session_tokens + data.budget.measured_model_tokens;

  return (
    <div className="overview-workspace">
      <PageHeader
        title={t("overview.title")}
        description={t("overview.description")}
        meta={
          <>
            <span>
              {t("overview.meta.coreSnapshot", {
                date: formatTimestamp(data.as_of),
              })}
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              {t("overview.meta.workers", { count: data.runtime.workers })}
            </span>
          </>
        }
        actions={
          <>
            <Link className={buttonClass("secondary")} to="/agents">
              {t("overview.actions.openAgents")}
            </Link>
            <Link className={buttonClass("quiet")} to="/inbox">
              {t("overview.actions.openDecisions")}
            </Link>
            <Link className={buttonClass("quiet")} to="/runtime">
              {t("overview.actions.runtime")}
            </Link>
          </>
        }
      />

      {error && <p className="error">{error}</p>}

      <section
        className="overview-metrics"
        aria-label={t("overview.summary.aria")}
      >
        <Metric
          label={t("overview.metric.authorizedCampaigns")}
          value={data.counts.authorized_campaigns}
          detail={t("overview.metric.authorizedCampaignsDetail", {
            count: data.counts.campaigns_awaiting_authorization,
          })}
          to="/campaigns"
          openLabel={t("overview.metric.open")}
          openAria={t("overview.metric.openAria", {
            label: t("overview.metric.authorizedCampaigns"),
          })}
        />
        <Metric
          label={t("overview.metric.runningAgents")}
          value={data.counts.active_agents}
          detail={
            data.counts.unresolved_sessions === 1
              ? t("overview.metric.runningAgentsDetailOne")
              : t("overview.metric.runningAgentsDetail", {
                  count: data.counts.unresolved_sessions,
                })
          }
          to="/agents"
          openLabel={t("overview.metric.open")}
          openAria={t("overview.metric.openAria", {
            label: t("overview.metric.runningAgents"),
          })}
        />
        <Metric
          label={t("overview.metric.pendingDecisions")}
          value={data.counts.pending_decisions}
          detail={t("overview.metric.pendingDecisionsDetail")}
          to="/inbox"
          openLabel={t("overview.metric.open")}
          openAria={t("overview.metric.openAria", {
            label: t("overview.metric.pendingDecisions"),
          })}
        />
        <Metric
          label={t("overview.metric.blockedSignals")}
          value={data.counts.blocked_signals}
          detail={t("overview.metric.blockedSignalsDetail")}
          to="/campaigns"
          openLabel={t("overview.metric.open")}
          openAria={t("overview.metric.openAria", {
            label: t("overview.metric.blockedSignals"),
          })}
        />
        <Metric
          label={t("overview.metric.acceptedCandidates")}
          value={accepted}
          detail={t("overview.metric.acceptedDetail", {
            count: data.counts.candidates,
          })}
          to="/campaigns"
          openLabel={t("overview.metric.open")}
          openAria={t("overview.metric.openAria", {
            label: t("overview.metric.acceptedCandidates"),
          })}
        />
        <Metric
          label={t("overview.metric.measuredSpend")}
          value={formatCostMicros(measuredCost)}
          detail={t("overview.metric.measuredTokens", {
            tokens: formatNumber(measuredTokens),
          })}
          to="/runtime"
          openLabel={t("overview.metric.open")}
          openAria={t("overview.metric.openAria", {
            label: t("overview.metric.measuredSpend"),
          })}
        />
      </section>

      <div className="overview-grid">
        <Panel className="overview-panel">
          <div className="section-heading">
            <h2>{t("overview.gate.title")}</h2>
            <Link to="/campaigns">{t("overview.gate.all")}</Link>
          </div>
          {gateEntries.length === 0 ? (
            <EmptyState
              title={t("overview.gate.emptyTitle")}
              description={t("overview.gate.emptyDescription")}
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
              <dt>{t("overview.facts.unmeasuredSessions")}</dt>
              <dd>{data.budget.unmeasured_sessions}</dd>
            </div>
            <div>
              <dt>{t("overview.facts.unmeasuredModelCalls")}</dt>
              <dd>{data.budget.unmeasured_model_calls}</dd>
            </div>
            <div>
              <dt>{t("overview.facts.unmeasuredBound")}</dt>
              <dd>
                {formatCostMicros(data.budget.unmeasured_bound_cost_micros)}
              </dd>
            </div>
            <div>
              <dt>{t("overview.facts.core")}</dt>
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
            <h2>{t("overview.decisions.title")}</h2>
            <Link to="/inbox">{t("overview.decisions.open")}</Link>
          </div>
          {data.recent_decisions.length === 0 ? (
            <EmptyState
              title={t("overview.decisions.emptyTitle")}
              description={t("overview.decisions.emptyDescription")}
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
            <h2>{t("overview.accepted.title")}</h2>
            <Link to="/campaigns">{t("overview.accepted.campaigns")}</Link>
          </div>
          {data.recent_accepted.length === 0 ? (
            <EmptyState
              title={t("overview.accepted.emptyTitle")}
              description={t("overview.accepted.emptyDescription")}
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
            <h2>{t("overview.runtime.title")}</h2>
            <Link to="/runtime">{t("overview.runtime.inspect")}</Link>
          </div>
          <dl className="overview-facts">
            <div>
              <dt>{t("overview.runtime.activeAgents")}</dt>
              <dd>{data.runtime.active_agents}</dd>
            </div>
            <div>
              <dt>{t("overview.runtime.unresolvedSessions")}</dt>
              <dd>{data.runtime.unresolved_sessions}</dd>
            </div>
            <div>
              <dt>{t("overview.runtime.unresolvedModelCalls")}</dt>
              <dd>{data.runtime.unresolved_model_calls}</dd>
            </div>
            <div>
              <dt>{t("overview.runtime.telemetry")}</dt>
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
          <h2>{t("overview.recentEvents.title")}</h2>
          <span className="muted">
            {t("overview.recentEvents.shown", {
              count: data.recent_events.length,
            })}
          </span>
        </div>
        {data.recent_events.length === 0 ? (
          <EmptyState
            title={t("overview.recentEvents.emptyTitle")}
            description={t("overview.recentEvents.emptyDescription")}
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
