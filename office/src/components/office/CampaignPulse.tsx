import { Code2, ScanEye, Search, ShieldCheck, UserRound } from "lucide-react";
import type {
  Campaign,
  CampaignDetail as CampaignDetailData,
  Gate,
  LeadRequestProjection,
  ProviderSession,
  QualityReceipt,
} from "../../coreApi";
import { formatTimestamp, shortId } from "../../format";
import { useI18n } from "../../i18n";
import { StatusBadge } from "./StatusBadge";

interface AgentRow {
  id: string;
  name: string;
  detail: string;
  status: string;
  icon: typeof UserRound;
}

function latestReceipt(
  detail: CampaignDetailData,
  receipts: Record<string, QualityReceipt[]>,
  role: "verifier" | "reviewer",
): QualityReceipt | null {
  let latest: QualityReceipt | null = null;
  for (const candidate of detail.candidates) {
    for (const receipt of receipts[candidate.id] ?? []) {
      if (
        receipt.role === role &&
        (latest === null ||
          receipt.received_at.localeCompare(latest.received_at) > 0)
      ) {
        latest = receipt;
      }
    }
  }
  return latest;
}

function latestGate(
  detail: CampaignDetailData,
  gates: Record<string, Gate | null>,
): Gate | null {
  let latest: Gate | null = null;
  for (const candidate of detail.candidates) {
    const gate = gates[candidate.id];
    if (
      gate &&
      (latest === null || gate.decided_at.localeCompare(latest.decided_at) > 0)
    ) {
      latest = gate;
    }
  }
  return latest;
}

/**
 * Campaign pulse is a read-only projection of recorded Core state. It never
 * derives success from a model response, a fixture or an assumed agent action.
 */
export function CampaignPulse({
  detail,
  gates,
  receipts,
  sessions,
  lead,
}: {
  detail: CampaignDetailData;
  gates: Record<string, Gate | null>;
  receipts: Record<string, QualityReceipt[]>;
  sessions: ProviderSession[];
  lead: LeadRequestProjection | null;
}) {
  const { t, formatNumber } = useI18n();
  const campaign: Campaign = detail.campaign;
  const verifier = latestReceipt(detail, receipts, "verifier");
  const reviewer = latestReceipt(detail, receipts, "reviewer");
  const gate = latestGate(detail, gates);
  const latestAttempt = detail.attempts.at(-1);
  const latestRun = detail.runs.at(-1);
  const activeSession = sessions.find((session) =>
    ["ACTIVE", "INTENT"].includes(session.state ?? ""),
  );

  const agents: AgentRow[] = [
    {
      id: "lead",
      name: t("campaignPulse.agent.lead"),
      detail: t("campaignPulse.agent.leadDetail"),
      status: lead?.request.state ?? campaign.status,
      icon: UserRound,
    },
    {
      id: "assessor",
      name: t("campaignPulse.agent.assessor"),
      detail: t("campaignPulse.agent.assessorDetail"),
      status: lead?.assessment ? "COMPLETED" : "PENDING",
      icon: Search,
    },
    {
      id: "implementer",
      name: t("campaignPulse.agent.implementer"),
      detail: t("campaignPulse.agent.implementerDetail"),
      status:
        latestAttempt?.status ??
        latestRun?.status ??
        activeSession?.state ??
        "PENDING",
      icon: Code2,
    },
    {
      id: "reviewer",
      name: t("campaignPulse.agent.reviewer"),
      detail: t("campaignPulse.agent.reviewerDetail"),
      status: reviewer?.result ?? "PENDING",
      icon: ScanEye,
    },
    {
      id: "verifier",
      name: t("campaignPulse.agent.verifier"),
      detail: t("campaignPulse.agent.verifierDetail"),
      status: verifier?.result ?? "PENDING",
      icon: ShieldCheck,
    },
  ];

  const completedTasks = detail.tasks.filter((task) =>
    ["SUCCEEDED", "COMPLETED", "ACCEPTED"].includes(task.status),
  ).length;
  const totalTasks = detail.tasks.length;
  const acceptedCandidates = detail.candidates.filter(
    (candidate) => gates[candidate.id]?.outcome === "ACCEPTED",
  ).length;
  const progressTotal = totalTasks > 0 ? totalTasks : detail.candidates.length;
  const progressDone = totalTasks > 0 ? completedTasks : acceptedCandidates;
  const percent =
    progressTotal === 0 ? 0 : Math.round((progressDone / progressTotal) * 100);
  const recentEvents = detail.events.slice(-4).reverse();

  return (
    <aside className="campaign-pulse" aria-label={t("campaignPulse.aria")}>
      <section className="campaign-pulse-section">
        <div className="campaign-pulse-heading">
          <h2>{t("campaignPulse.progress")}</h2>
          <span className="muted">
            {progressTotal > 0
              ? t("campaignPulse.completed", {
                  done: formatNumber(progressDone),
                  total: formatNumber(progressTotal),
                })
              : t("campaignPulse.notStarted")}
          </span>
        </div>
        <div
          className="campaign-progress"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          aria-label={t("campaignPulse.progress")}
        >
          <span style={{ width: `${percent}%` }} />
        </div>
        <p className="campaign-progress-caption">{percent}%</p>
      </section>

      <section className="campaign-pulse-section">
        <div className="campaign-pulse-heading">
          <h2>{t("campaignPulse.agents")}</h2>
        </div>
        <ul className="campaign-agent-list">
          {agents.map((agent) => {
            const Icon = agent.icon;
            return (
              <li key={agent.id}>
                <span className="campaign-agent-icon" aria-hidden="true">
                  <Icon size={15} />
                </span>
                <span className="campaign-agent-copy">
                  <strong>{agent.name}</strong>
                  <span>{agent.detail}</span>
                </span>
                <StatusBadge status={agent.status} />
              </li>
            );
          })}
        </ul>
      </section>

      <section className="campaign-pulse-section">
        <div className="campaign-pulse-heading">
          <h2>{t("campaignPulse.gate")}</h2>
        </div>
        {gate ? (
          <div className="campaign-pulse-gate">
            <StatusBadge status={gate.outcome} />
            <span className="muted">
              {t("campaignPulse.decided", {
                date: formatTimestamp(gate.decided_at),
              })}
            </span>
          </div>
        ) : (
          <p className="muted">{t("campaignPulse.gatePending")}</p>
        )}
      </section>

      <section className="campaign-pulse-section">
        <div className="campaign-pulse-heading">
          <h2>{t("campaignPulse.activity")}</h2>
        </div>
        {recentEvents.length === 0 ? (
          <p className="muted">{t("campaignPulse.noActivity")}</p>
        ) : (
          <ol
            className="campaign-activity-list"
            aria-label={t("campaignPulse.activity")}
            reversed
          >
            {recentEvents.map((event) => (
              <li key={event.sequence}>
                <span className="campaign-activity-dot" aria-hidden="true" />
                <div>
                  <strong>{event.kind}</strong>
                  <span>
                    {shortId(event.subject_id)} ·{" "}
                    {formatTimestamp(event.recorded_at)}
                  </span>
                  <p>{event.detail}</p>
                </div>
              </li>
            ))}
          </ol>
        )}
      </section>
    </aside>
  );
}
