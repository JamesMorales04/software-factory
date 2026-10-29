import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams, useSearchParams } from "react-router-dom";
import { CancelCampaign } from "../CancelCampaign";
import { CandidateDiff } from "../components/office/CandidateDiff";
import {
  CandidateIdentity,
  QualityGatePanel,
  ReceiptEvidenceTable,
  ReceiptModelProvenance,
  ReviewFindings,
} from "../components/office/CandidateEvidence";
import { CandidatePullRequest } from "../components/office/CandidatePullRequest";
import { ParticipantSessionList } from "../components/office/ParticipantSessionList";
import { StatusBadge } from "../components/office/StatusBadge";
import { WorkGraphPanel } from "../components/office/WorkGraph";
import { Button, buttonClass } from "../components/ui/Button";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import { type TabItem, Tabs } from "../components/ui/Tabs";
import {
  type Action,
  type Campaign,
  type CampaignDetail as CampaignDetailData,
  CoreApiError,
  coreApi,
  type Gate,
  type LeadRequestProjection,
  type Project,
  type ProviderSession,
  type QualityReceipt,
  type QualityRole,
} from "../coreApi";
import { formatCostMicros, formatTimestamp, shortId } from "../format";
import { useI18n } from "../i18n";

interface LoadedCampaign {
  detail: CampaignDetailData;
  gates: Record<string, Gate | null>;
  receipts: Record<string, QualityReceipt[]>;
  sessions: ProviderSession[];
  action: Action | null;
  project: Project | null;
  lead: LeadRequestProjection | null;
}

const TAB_IDS = [
  "overview",
  "workgraph",
  "execution",
  "candidates",
  "evidence",
  "gate",
  "budget",
  "events",
] as const;
type TabId = (typeof TAB_IDS)[number];

function resolveTab(value: string | null): TabId {
  return TAB_IDS.includes(value as TabId) ? (value as TabId) : "overview";
}

function countStatuses(items: { status: string }[]): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const item of items) {
    counts[item.status] = (counts[item.status] ?? 0) + 1;
  }
  return counts;
}

function summarizeCounts(
  counts: Record<string, number>,
  t: ReturnType<typeof useI18n>["t"],
): string {
  const entries = Object.entries(counts);
  if (entries.length === 0) return t("campaignDetail.summary.none");
  return entries
    .map(
      ([status, count]) =>
        `${count} ${status.toLowerCase().replaceAll("_", " ")}`,
    )
    .join(" · ");
}

function summarizeReceipts(
  candidateIds: string[],
  receipts: Record<string, QualityReceipt[]>,
  role: QualityRole,
  t: ReturnType<typeof useI18n>["t"],
): string {
  if (candidateIds.length === 0) return t("campaignDetail.waitingCandidate");
  const results: Record<string, number> = {};
  let recorded = 0;
  for (const candidateId of candidateIds) {
    const receipt = (receipts[candidateId] ?? []).find(
      (item) => item.role === role,
    );
    if (receipt) {
      recorded += 1;
      results[receipt.result] = (results[receipt.result] ?? 0) + 1;
    }
  }
  const detail = Object.entries(results)
    .map(([result, count]) => `${count} ${result}`)
    .join(" · ");
  return t("campaignDetail.receiptsRecorded", {
    recorded,
    total: candidateIds.length,
    detail: detail ? ` · ${detail}` : "",
  });
}

function executionModeLabel(
  mode: string,
  t: ReturnType<typeof useI18n>["t"],
): string {
  if (mode === "FIXTURE") return t("campaignDetail.executionMode.fixture");
  if (mode === "TRUSTED_LOCAL_PROVIDER")
    return t("campaignDetail.executionMode.trustedLocal");
  return mode;
}

function formatStringList(
  values: string[] | null | undefined,
  t: ReturnType<typeof useI18n>["t"],
): string {
  if (!Array.isArray(values)) return t("common.none");
  const present = values.filter((value) => value !== "");
  return present.length === 0 ? t("common.none") : present.join(", ");
}

function summarizeGates(
  candidateIds: string[],
  gates: Record<string, Gate | null>,
  t: ReturnType<typeof useI18n>["t"],
): string {
  if (candidateIds.length === 0) return t("campaignDetail.waitingCandidate");
  const outcomes: Record<string, number> = {};
  let decided = 0;
  for (const candidateId of candidateIds) {
    const gate = gates[candidateId];
    if (gate) {
      decided += 1;
      outcomes[gate.outcome] = (outcomes[gate.outcome] ?? 0) + 1;
    }
  }
  const detail = Object.entries(outcomes)
    .map(([outcome, count]) => `${count} ${outcome}`)
    .join(" · ");
  return t("campaignDetail.gatesDecided", {
    decided,
    total: candidateIds.length,
    detail: detail ? ` · ${detail}` : " · PENDING",
  });
}

function BudgetRow({
  label,
  value,
  warning = false,
}: {
  label: string;
  value: string;
  warning?: boolean;
}) {
  return (
    <>
      <dt>{label}</dt>
      <dd className={warning ? "warning" : undefined}>{value}</dd>
    </>
  );
}

function TabPanel({ id, children }: { id: TabId; children: React.ReactNode }) {
  return (
    <div
      className="campaign-tab-panel"
      id={`panel-${id}`}
      role="tabpanel"
      aria-labelledby={`tab-${id}`}
    >
      {children}
    </div>
  );
}

export function CampaignDetail() {
  const { t } = useI18n();
  const { campaignId = "" } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const tab = resolveTab(searchParams.get("tab"));
  const selectedUnitId = searchParams.get("unit");
  const [loaded, setLoaded] = useState<LoadedCampaign | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);
  const alive = useRef(true);

  const selectTab = (next: string) => {
    const params = new URLSearchParams(searchParams);
    params.set("tab", resolveTab(next));
    setSearchParams(params);
  };

  const selectUnit = (unitId: string | null) => {
    const params = new URLSearchParams(searchParams);
    params.set("tab", "workgraph");
    if (unitId) params.set("unit", unitId);
    else params.delete("unit");
    setSearchParams(params);
  };

  const load = useCallback(async () => {
    const detail = await coreApi.getCampaignDetail(campaignId);
    const [actions, projects, leadRequests, allSessions, candidateData] =
      await Promise.all([
        coreApi.listActions(),
        coreApi.listProjects(),
        coreApi.listLeadRequests(),
        coreApi.listProviderSessions(),
        Promise.all(
          detail.candidates.map(async (candidate) => {
            const [gate, receipts] = await Promise.all([
              coreApi.getGate(candidate.id),
              coreApi.getCandidateReceipts(candidate.id),
            ]);
            return { candidateId: candidate.id, gate, receipts };
          }),
        ),
      ]);
    const action =
      actions.find((item) => item.id === detail.campaign.action_id) ?? null;
    const project = action
      ? (projects.find((item) => item.id === action.project_id) ?? null)
      : null;
    const leadRecord =
      leadRequests.find((item) => item.campaign_id === campaignId) ?? null;
    const lead = leadRecord
      ? await coreApi.getLeadRequest(leadRecord.id)
      : null;
    const attemptIds = new Set(detail.attempts.map((attempt) => attempt.id));
    const sessions = allSessions.filter((session) =>
      attemptIds.has(session.attempt_id),
    );
    const gates: Record<string, Gate | null> = {};
    const receipts: Record<string, QualityReceipt[]> = {};
    for (const item of candidateData) {
      gates[item.candidateId] = item.gate;
      receipts[item.candidateId] = item.receipts;
    }
    if (alive.current) {
      setLoaded({
        detail,
        gates,
        receipts,
        sessions,
        action,
        project,
        lead,
      });
      setLastLoadedAt(new Date().toISOString());
      setError(null);
    }
  }, [campaignId]);

  useEffect(() => {
    alive.current = true;
    async function refresh() {
      try {
        await load();
      } catch (cause) {
        if (alive.current) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : t("campaignDetail.error.load"),
          );
        }
      }
    }
    void refresh();
    const timer = setInterval(() => void refresh(), 5000);
    return () => {
      alive.current = false;
      clearInterval(timer);
    };
  }, [load, t]);

  async function handleCancelled(campaign: Campaign) {
    setNotice(
      t("campaignDetail.notice.cancelled", {
        id: shortId(campaign.id),
        status: campaign.status,
      }),
    );
    try {
      await load();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("campaignDetail.error.refresh"),
      );
    }
  }

  if (error && !loaded) {
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
  if (!loaded) return <LoadingState label={t("campaignDetail.loading")} />;

  const { detail, gates, receipts, sessions, action, project, lead } = loaded;
  const campaign = detail.campaign;
  const authorization = detail.authorization;
  const candidateIds = detail.candidates.map((candidate) => candidate.id);
  const taskCounts = countStatuses(detail.tasks);
  const runCounts = countStatuses(detail.runs);
  const attemptCounts = countStatuses(detail.attempts);
  const cancelEvent = detail.events.find(
    (event) => event.kind === "campaign.cancelled",
  );
  const leadBlocked =
    lead?.request.state === "BLOCKED" && lead.request.failure_reason !== null;
  const leadFailed = lead?.request.state === "FAILED";
  const unresolvedSessions = sessions.filter((session) =>
    ["INTENT", "ACTIVE", "CANCEL_REQUESTED", "ORPHANED"].includes(
      session.state,
    ),
  );
  const unconfirmedTermination = sessions.filter(
    (session) =>
      session.state === "CANCEL_REQUESTED" && !session.cleanup_receipt,
  );
  const measuredSessions = sessions.filter((session) => session.usage_known);
  const unmeasuredSessions = sessions.filter((session) => !session.usage_known);
  const totalInput = measuredSessions.reduce(
    (sum, session) => sum + session.input_tokens,
    0,
  );
  const totalOutput = measuredSessions.reduce(
    (sum, session) => sum + session.output_tokens,
    0,
  );
  const totalCost = measuredSessions.reduce(
    (sum, session) => sum + session.cost_micros,
    0,
  );
  const unmeasuredBound = unmeasuredSessions.reduce(
    (sum, session) => sum + session.cost_micros,
    0,
  );
  const turnRecords = sessions.filter(
    (session) => session.turn_id !== "",
  ).length;
  // Only a historical V1 row has productivity ceilings to compare against.
  // V2 policy.operations/progress are per-operation bounds, not Campaign caps.
  const legacyCaps = authorization?.legacy ?? null;
  const overBudget: string[] = [];
  if (legacyCaps) {
    if (legacyCaps.maxSessions > 0 && sessions.length > legacyCaps.maxSessions)
      overBudget.push("sessions");
    if (legacyCaps.maxTurns > 0 && turnRecords > legacyCaps.maxTurns)
      overBudget.push("turns");
    if (legacyCaps.maxInputTokens > 0 && totalInput > legacyCaps.maxInputTokens)
      overBudget.push("input tokens");
    if (
      legacyCaps.maxOutputTokens > 0 &&
      totalOutput > legacyCaps.maxOutputTokens
    )
      overBudget.push("output tokens");
    if (legacyCaps.maxCostMicros > 0 && totalCost > legacyCaps.maxCostMicros)
      overBudget.push("cost");
  }

  const latestRun = detail.runs.at(-1) ?? null;
  const latestAttempt = detail.attempts.at(-1) ?? null;
  const latestCandidate = detail.candidates.at(-1) ?? null;
  const latestGate = latestCandidate
    ? (gates[latestCandidate.id] ?? null)
    : null;
  const pendingGates = candidateIds.filter((id) => !gates[id]).length;
  const attention: string[] = [];
  if (campaign.status === "BLOCKED_AUTHORIZATION") {
    attention.push(t("campaignDetail.attention.authorization"));
  }
  if (leadBlocked) attention.push(t("campaignDetail.attention.leadBlocked"));
  if (leadFailed) attention.push(t("campaignDetail.attention.leadFailed"));
  if (detail.runs.some((run) => ["FAILED", "REJECTED"].includes(run.status))) {
    attention.push(t("campaignDetail.attention.runFailed"));
  }
  if (
    candidateIds.some((id) =>
      ["REPAIR_REQUIRED", "BLOCKED", "HUMAN_REQUIRED", "NOT_TESTED"].includes(
        gates[id]?.outcome ?? "",
      ),
    )
  ) {
    attention.push(t("campaignDetail.attention.gateReview"));
  }
  if (unresolvedSessions.length > 0) {
    attention.push(t("campaignDetail.attention.sessionCleanup"));
  }
  if (unconfirmedTermination.length > 0) {
    attention.push(t("campaignDetail.attention.termination"));
  }
  if (overBudget.length > 0) {
    attention.push(
      t("campaignDetail.attention.overBudget", {
        items: overBudget.join(", "),
      }),
    );
  }

  const workflow = [
    {
      stage: t("campaignDetail.workflow.stage.project"),
      reference: project
        ? shortId(project.id)
        : t("campaignDetail.workflow.notLinked"),
      state: project
        ? project.repository
        : t("campaignDetail.workflow.projectMissing"),
    },
    {
      stage: t("campaignDetail.workflow.stage.action"),
      reference: action
        ? shortId(action.id)
        : t("campaignDetail.workflow.notFound"),
      state: action
        ? t("campaignDetail.workflow.actionState", {
            status: action.status,
            risk: action.risk,
          })
        : t("campaignDetail.workflow.actionMissing"),
    },
    {
      stage: t("campaignDetail.workflow.stage.lead"),
      reference: lead
        ? shortId(lead.request.id)
        : t("campaignDetail.workflow.notLinked"),
      state: lead
        ? lead.request.state
        : t("campaignDetail.workflow.leadMissing"),
    },
    {
      stage: t("campaignDetail.workflow.stage.assessment"),
      reference: lead?.assessment
        ? shortId(lead.assessment.id)
        : t("campaignDetail.workflow.missing"),
      state: lead?.assessment
        ? t("campaignDetail.workflow.assessmentState", {
            mechanism: lead.assessment.mechanism,
            date: formatTimestamp(lead.assessment.created_at),
          })
        : t("campaignDetail.workflow.assessmentMissing"),
    },
    {
      stage: t("campaignDetail.workflow.stage.plan"),
      reference: lead?.plan
        ? shortId(lead.plan.plan_hash, 12)
        : t("campaignDetail.workflow.notFrozen"),
      state: lead?.plan
        ? t("campaignDetail.workflow.planState", {
            hash: shortId(lead.plan.config_hash, 12),
          })
        : t("campaignDetail.workflow.planMissing"),
    },
    {
      stage: t("campaignDetail.workflow.stage.campaign"),
      reference: shortId(campaign.id),
      state: campaign.status,
    },
    {
      stage: t("campaignDetail.workflow.stage.workgraph"),
      reference:
        detail.tasks.length === 1
          ? t("campaignDetail.workflow.unitOne", {
              count: detail.tasks.length,
            })
          : t("campaignDetail.workflow.units", {
              count: detail.tasks.length,
            }),
      state: summarizeCounts(taskCounts, t),
    },
    {
      stage: t("campaignDetail.workflow.stage.runs"),
      reference: `${detail.runs.length}`,
      state: summarizeCounts(runCounts, t),
    },
    {
      stage: t("campaignDetail.workflow.stage.attempts"),
      reference: `${detail.attempts.length}`,
      state: summarizeCounts(attemptCounts, t),
    },
    {
      stage: t("campaignDetail.workflow.stage.candidates"),
      reference: `${detail.candidates.length}`,
      state:
        detail.candidates.length === 0
          ? t("campaignDetail.workflow.noneRecorded")
          : pendingGates === 1
            ? t("campaignDetail.workflow.gatePendingOne", {
                count: pendingGates,
              })
            : t("campaignDetail.workflow.gatesPending", {
                count: pendingGates,
              }),
    },
    {
      stage: t("campaignDetail.workflow.stage.verifier"),
      reference: summarizeReceipts(candidateIds, receipts, "verifier", t),
      state: t("campaignDetail.workflow.verifierState"),
    },
    {
      stage: t("campaignDetail.workflow.stage.reviewer"),
      reference: summarizeReceipts(candidateIds, receipts, "reviewer", t),
      state: t("campaignDetail.workflow.reviewerState"),
    },
    {
      stage: t("campaignDetail.workflow.stage.gate"),
      reference: summarizeGates(candidateIds, gates, t),
      state: t("campaignDetail.workflow.gateState"),
    },
  ];

  const tabs: TabItem[] = [
    { id: "overview", label: t("campaignDetail.tabs.overview") },
    {
      id: "workgraph",
      label: t("campaignDetail.tabs.workgraph"),
      count: detail.tasks.length,
    },
    {
      id: "execution",
      label: t("campaignDetail.tabs.execution"),
      count: detail.runs.length + detail.attempts.length,
    },
    {
      id: "candidates",
      label: t("campaignDetail.tabs.candidates"),
      count: detail.candidates.length,
    },
    {
      id: "evidence",
      label: t("campaignDetail.tabs.evidence"),
      count: candidateIds.reduce(
        (sum, id) => sum + (receipts[id]?.length ?? 0),
        0,
      ),
    },
    {
      id: "gate",
      label: t("campaignDetail.tabs.gate"),
      count: candidateIds.filter((id) => gates[id]).length,
    },
    { id: "budget", label: t("campaignDetail.tabs.budget") },
    {
      id: "events",
      label: t("campaignDetail.tabs.events"),
      count: detail.events.length,
    },
  ];

  return (
    <div className="campaign-workspace">
      <PageHeader
        title={t("campaignDetail.title", { id: shortId(campaign.id) })}
        description={
          action?.objective ?? t("campaignDetail.descriptionFallback")
        }
        meta={
          <>
            <StatusBadge status={campaign.status} />
            <span>
              {t("campaignDetail.meta.action")}{" "}
              <code>{shortId(campaign.action_id)}</code>
            </span>
            <span>
              {authorization
                ? executionModeLabel(authorization.executionMode, t)
                : t("campaignDetail.meta.authorizationMissing")}
            </span>
            <span>
              {t("campaignDetail.meta.latestGate")}{" "}
              {latestGate ? (
                <StatusBadge status={latestGate.outcome} />
              ) : (
                t("campaignDetail.pending")
              )}
            </span>
            <span>
              {t("campaignDetail.meta.fetched", {
                date: lastLoadedAt ? formatTimestamp(lastLoadedAt) : "—",
              })}
            </span>
          </>
        }
        actions={
          <>
            <CancelCampaign campaign={campaign} onCancelled={handleCancelled} />
            <Link className={buttonClass("quiet", "sm")} to="/campaigns">
              {t("campaignDetail.back")}
            </Link>
          </>
        }
      />
      {error && (
        <div className="ui-callout ui-callout--danger" role="alert">
          {error} {t("campaignDetail.error.stale")}
        </div>
      )}
      {notice && <p className="notice">{notice}</p>}

      <Panel className="campaign-summary">
        <div className="campaign-summary-head">
          <h2>{t("campaignDetail.summary.title")}</h2>
          {attention.length === 0 ? (
            <span className="muted">
              {t("campaignDetail.summary.noAttention")}
            </span>
          ) : (
            <StatusBadge
              status="HUMAN_REQUIRED"
              label={
                attention.length === 1
                  ? t("campaignDetail.summary.attentionOne", {
                      count: attention.length,
                    })
                  : t("campaignDetail.summary.attention", {
                      count: attention.length,
                    })
              }
            />
          )}
        </div>
        <dl className="overview-facts">
          <div>
            <dt>{t("campaignDetail.summary.project")}</dt>
            <dd>
              {project ? (
                <Link to="/projects">{shortId(project.id)}</Link>
              ) : (
                t("campaignDetail.workflow.notLinked")
              )}
            </dd>
          </div>
          <div>
            <dt>{t("campaignDetail.summary.action")}</dt>
            <dd>
              <Link to={`/actions/${campaign.action_id}`}>
                {shortId(campaign.action_id)}
              </Link>
            </dd>
          </div>
          <div>
            <dt>{t("campaignDetail.summary.frozenPlan")}</dt>
            <dd>
              {lead?.plan
                ? shortId(lead.plan.plan_hash, 12)
                : t("campaignDetail.summary.notRecorded")}
            </dd>
          </div>
          <div>
            <dt>{t("campaignDetail.summary.authorization")}</dt>
            <dd>
              {authorization
                ? authorization.riskClass
                : t("campaignDetail.summary.notRecorded")}
            </dd>
          </div>
          <div>
            <dt>{t("campaignDetail.summary.latestRun")}</dt>
            <dd>
              {latestRun ? (
                <>
                  <code>{shortId(latestRun.id)}</code>{" "}
                  <StatusBadge status={latestRun.status} />
                </>
              ) : (
                t("campaignDetail.summary.none")
              )}
            </dd>
          </div>
          <div>
            <dt>{t("campaignDetail.summary.latestAttempt")}</dt>
            <dd>
              {latestAttempt ? (
                <>
                  <code>{shortId(latestAttempt.id)}</code>{" "}
                  <StatusBadge status={latestAttempt.status} />
                </>
              ) : (
                t("campaignDetail.summary.none")
              )}
            </dd>
          </div>
          <div>
            <dt>{t("campaignDetail.summary.latestCandidate")}</dt>
            <dd>
              {latestCandidate
                ? shortId(latestCandidate.id)
                : t("campaignDetail.summary.none")}
            </dd>
          </div>
          <div>
            <dt>{t("campaignDetail.summary.latestGate")}</dt>
            <dd>
              {latestGate ? (
                <StatusBadge status={latestGate.outcome} />
              ) : (
                t("campaignDetail.pending")
              )}
            </dd>
          </div>
        </dl>
        {attention.length > 0 && (
          <div className="ui-callout ui-callout--attention" role="status">
            <ul className="attention-list">
              {attention.map((reason) => (
                <li key={reason}>{reason}</li>
              ))}
            </ul>
          </div>
        )}
      </Panel>

      <Tabs
        items={tabs}
        value={tab}
        onChange={selectTab}
        label={t("campaignDetail.tabs.label")}
      />

      {tab === "overview" && (
        <TabPanel id="overview">
          <Panel>
            <div className="section-heading">
              <h2>{t("campaignDetail.overview.lifecycleTitle")}</h2>
              <span className="muted">
                {t("campaignDetail.overview.lifecycleHint")}
              </span>
            </div>
            <div className="table-responsive">
              <table data-testid="workflow-projection">
                <thead>
                  <tr>
                    <th>{t("campaignDetail.overview.stage")}</th>
                    <th>{t("campaignDetail.overview.reference")}</th>
                    <th>{t("campaignDetail.overview.coreState")}</th>
                  </tr>
                </thead>
                <tbody>
                  {workflow.map((row) => (
                    <tr key={row.stage}>
                      <th scope="row">{row.stage}</th>
                      <td data-label={t("campaignDetail.overview.reference")}>
                        {row.reference}
                      </td>
                      <td data-label={t("campaignDetail.overview.coreState")}>
                        {row.state}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {lead && lead.model_calls.length > 0 && (
              <div data-testid="lead-model-call-provenance">
                <h3>{t("campaignDetail.overview.leadModelCalls")}</h3>
                <p className="muted">
                  {lead.model_calls
                    .map((call) =>
                      t("campaignDetail.overview.leadModelCall", {
                        route: `${call.provider}/${call.model}`,
                        reservation: shortId(call.reservation_id),
                        state: call.state,
                        usage: call.usage_known
                          ? t("campaignDetail.overview.callUsage", {
                              input: call.input_tokens.toLocaleString(),
                              output: call.output_tokens.toLocaleString(),
                              total: call.total_tokens.toLocaleString(),
                              cost: formatCostMicros(call.cost_micros),
                            })
                          : t("campaignDetail.overview.usageUnmeasured"),
                      }),
                    )
                    .join(" · ")}
                </p>
              </div>
            )}
          </Panel>
        </TabPanel>
      )}

      {tab === "workgraph" && (
        <TabPanel id="workgraph">
          <Panel>
            <div className="section-heading">
              <h2>
                {detail.tasks.length === 1
                  ? t("campaignDetail.workgraph.titleOne", {
                      count: detail.tasks.length,
                    })
                  : t("campaignDetail.workgraph.title", {
                      count: detail.tasks.length,
                    })}
              </h2>
              <span className="muted">{summarizeCounts(taskCounts, t)}</span>
            </div>
            <WorkGraphPanel
              tasks={detail.tasks}
              campaignId={campaign.id}
              selectedUnitId={selectedUnitId}
              onSelectUnit={selectUnit}
              sessions={sessions}
              gates={gates}
              receipts={receipts}
              stale={Boolean(error)}
              selectedUnitMissing={
                Boolean(selectedUnitId) &&
                !detail.tasks.some((task) => task.unit_id === selectedUnitId)
              }
            />
          </Panel>
        </TabPanel>
      )}

      {tab === "execution" && (
        <TabPanel id="execution">
          <Panel>
            <div className="section-heading">
              <h2>{t("campaignDetail.execution.title")}</h2>
              <span className="muted">
                {summarizeCounts(runCounts, t)} ·{" "}
                {summarizeCounts(attemptCounts, t)}
              </span>
            </div>
            {detail.runs.length === 0 ? (
              <EmptyState
                title={t("campaignDetail.execution.noRunsTitle")}
                description={t("campaignDetail.execution.noRunsDescription")}
              />
            ) : (
              <>
                <div className="table-responsive">
                  <table>
                    <thead>
                      <tr>
                        <th>{t("campaignDetail.execution.table.run")}</th>
                        <th>{t("campaignDetail.execution.table.status")}</th>
                        <th>{t("campaignDetail.execution.table.attempts")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {detail.runs.map((run) => (
                        <tr key={run.id}>
                          <td
                            data-label={t("campaignDetail.execution.table.run")}
                          >
                            <code>{shortId(run.id)}</code>
                          </td>
                          <td
                            data-label={t(
                              "campaignDetail.execution.table.status",
                            )}
                          >
                            <StatusBadge status={run.status} />
                          </td>
                          <td
                            data-label={t(
                              "campaignDetail.execution.table.attempts",
                            )}
                          >
                            {detail.attempts
                              .filter((attempt) => attempt.run_id === run.id)
                              .map(
                                (attempt) =>
                                  `${shortId(attempt.id)}:${attempt.status}`,
                              )
                              .join(", ") || "—"}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <h3>
                  {t("campaignDetail.execution.attemptsTitle", {
                    count: detail.attempts.length,
                  })}
                </h3>
                {detail.attempts.length === 0 ? (
                  <p className="muted">
                    {t("campaignDetail.execution.noAttempts")}
                  </p>
                ) : (
                  <div className="table-responsive">
                    <table>
                      <thead>
                        <tr>
                          <th>{t("campaignDetail.execution.table.attempt")}</th>
                          <th>{t("campaignDetail.execution.table.run")}</th>
                          <th>{t("campaignDetail.execution.table.status")}</th>
                          <th>{t("campaignDetail.execution.table.created")}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {detail.attempts.map((attempt) => (
                          <tr key={attempt.id}>
                            <td
                              data-label={t(
                                "campaignDetail.execution.table.attempt",
                              )}
                            >
                              <code>{shortId(attempt.id)}</code>
                            </td>
                            <td
                              data-label={t(
                                "campaignDetail.execution.table.run",
                              )}
                            >
                              <code>{shortId(attempt.run_id)}</code>
                            </td>
                            <td
                              data-label={t(
                                "campaignDetail.execution.table.status",
                              )}
                            >
                              <StatusBadge status={attempt.status} />
                            </td>
                            <td
                              className="muted"
                              data-label={t(
                                "campaignDetail.execution.table.created",
                              )}
                            >
                              {formatTimestamp(attempt.created_at)}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </>
            )}
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>{t("campaignDetail.execution.sessionsTitle")}</h2>
              <span className="muted">
                {t("campaignDetail.execution.sessionsHint")}
              </span>
            </div>
            <ParticipantSessionList
              sessions={sessions}
              emptyTitle={t("campaignDetail.execution.sessionsEmptyTitle")}
              emptyDescription={t(
                "campaignDetail.execution.sessionsEmptyDescription",
              )}
            />
          </Panel>
        </TabPanel>
      )}

      {tab === "candidates" && (
        <TabPanel id="candidates">
          {detail.candidates.length === 0 ? (
            <EmptyState
              title={t("campaignDetail.candidates.emptyTitle")}
              description={t("campaignDetail.candidates.emptyDescription")}
            />
          ) : (
            detail.candidates.map((candidate) => {
              const gate = gates[candidate.id] ?? null;
              return (
                <Panel key={candidate.id} data-testid="candidate-panel">
                  <div className="session-card-head">
                    <h2>
                      {t("campaignDetail.candidateHeading", {
                        id: shortId(candidate.id),
                      })}
                    </h2>
                    {gate ? (
                      <StatusBadge status={gate.outcome} />
                    ) : (
                      <StatusBadge
                        status="PENDING"
                        label={t("campaignDetail.candidates.gatePending")}
                      />
                    )}
                  </div>
                  <CandidateIdentity candidate={candidate} />
                  <CandidateDiff
                    campaignId={campaign.id}
                    candidateId={candidate.id}
                  />
                  {project && (
                    <CandidatePullRequest
                      campaignId={campaign.id}
                      candidateId={candidate.id}
                      projectId={project.id}
                      candidateCommitSha={candidate.commit_sha}
                    />
                  )}
                  <div className="campaign-actions">
                    <Button size="sm" onClick={() => selectTab("evidence")}>
                      {t("campaignDetail.candidates.viewEvidence")}
                    </Button>
                    <Button size="sm" onClick={() => selectTab("gate")}>
                      {t("campaignDetail.candidates.viewGate")}
                    </Button>
                  </div>
                </Panel>
              );
            })
          )}
        </TabPanel>
      )}

      {tab === "evidence" && (
        <TabPanel id="evidence">
          {detail.candidates.length === 0 ? (
            <EmptyState
              title={t("campaignDetail.evidence.emptyTitle")}
              description={t("campaignDetail.evidence.emptyDescription")}
            />
          ) : (
            detail.candidates.map((candidate) => {
              const candidateReceipts = receipts[candidate.id] ?? [];
              return (
                <Panel key={candidate.id}>
                  <div className="section-heading">
                    <h2>
                      {t("campaignDetail.candidateHeading", {
                        id: shortId(candidate.id),
                      })}
                    </h2>
                    <span className="muted">
                      {t("campaignDetail.evidence.independent")}
                    </span>
                  </div>
                  <ReceiptEvidenceTable receipts={candidateReceipts} />
                  <ReceiptModelProvenance receipts={candidateReceipts} />
                  <h3>{t("campaignDetail.evidence.reviewFindings")}</h3>
                  <ReviewFindings receipts={candidateReceipts} />
                </Panel>
              );
            })
          )}
        </TabPanel>
      )}

      {tab === "gate" && (
        <TabPanel id="gate">
          {detail.candidates.length === 0 ? (
            <EmptyState
              title={t("campaignDetail.gate.emptyTitle")}
              description={t("campaignDetail.gate.emptyDescription")}
            />
          ) : (
            detail.candidates.map((candidate) => (
              <Panel key={candidate.id}>
                <div className="section-heading">
                  <h2>
                    {t("campaignDetail.candidateHeading", {
                      id: shortId(candidate.id),
                    })}
                  </h2>
                  <StatusBadge
                    status={gates[candidate.id]?.outcome ?? "PENDING"}
                  />
                </div>
                <QualityGatePanel
                  gate={gates[candidate.id] ?? null}
                  candidateId={candidate.id}
                />
              </Panel>
            ))
          )}
        </TabPanel>
      )}

      {tab === "budget" && (
        <TabPanel id="budget">
          <Panel>
            <div className="section-heading">
              <h2>{t("campaignDetail.budget.title")}</h2>
              <span className="muted">{t("campaignDetail.budget.hint")}</span>
            </div>
            {authorization ? (
              <>
                <p>
                  {authorization.authorizationId} · {authorization.riskClass} ·{" "}
                  {executionModeLabel(authorization.executionMode, t)} ·{" "}
                  {t("campaignDetail.budget.actor", {
                    actor: authorization.actor,
                  })}
                </p>
                <p className="muted">{authorization.rationale}</p>
                <dl className="detail">
                  <BudgetRow
                    label={t("campaignDetail.budget.execution")}
                    value={executionModeLabel(authorization.executionMode, t)}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.schemaVersion")}
                    value={String(authorization.schemaVersion)}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.route")}
                    value={
                      authorization.executionMode === "FIXTURE"
                        ? authorization.provider || authorization.model
                          ? t("campaignDetail.budget.fixtureRoute", {
                              provider: authorization.provider,
                              model: authorization.model,
                              mode:
                                authorization.providerMode ||
                                t("campaignDetail.budget.modeNotSet"),
                            })
                          : t("campaignDetail.budget.fixtureNoProvider")
                        : authorization.provider || authorization.model
                          ? t("campaignDetail.budget.providerRoute", {
                              provider: authorization.provider,
                              model: authorization.model,
                              mode:
                                authorization.providerMode ||
                                t("campaignDetail.budget.modeNotSet"),
                            })
                          : t("campaignDetail.budget.providerNoRoute")
                    }
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.delegation")}
                    value={authorization.delegationId || t("common.notSet")}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.authorizedPaths")}
                    value={formatStringList(authorization.authorizedPaths, t)}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.allowedTools")}
                    value={formatStringList(authorization.allowedTools, t)}
                  />
                  {authorization.policy && (
                    <>
                      <BudgetRow
                        label={t("campaignDetail.budget.policyId")}
                        value={authorization.policy.policyId}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.monetaryEnvelope")}
                        value={authorization.policy.monetaryEnvelopeId}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.scopeHash")}
                        value={shortId(authorization.policy.scopeSha256, 16)}
                      />
                    </>
                  )}
                  <BudgetRow
                    label={t("campaignDetail.budget.attempts")}
                    value={t("campaignDetail.budget.observedAttempts", {
                      count: detail.attempts.length,
                    })}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.sessions")}
                    value={t("campaignDetail.budget.observedSessions", {
                      count: sessions.length,
                    })}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.turnRecords")}
                    value={t("campaignDetail.budget.turnRecordsObserved", {
                      count: turnRecords,
                    })}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.inputTokens")}
                    value={t("campaignDetail.budget.tokensMeasured", {
                      count: totalInput.toLocaleString(),
                    })}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.outputTokens")}
                    value={t("campaignDetail.budget.tokensMeasured", {
                      count: totalOutput.toLocaleString(),
                    })}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.cost")}
                    value={t("campaignDetail.budget.costMeasured", {
                      amount: formatCostMicros(totalCost),
                    })}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.unmeasuredSessions")}
                    value={t("campaignDetail.budget.unmeasuredOf", {
                      count: unmeasuredSessions.length,
                      total: sessions.length,
                    })}
                  />
                  <BudgetRow
                    label={t("campaignDetail.budget.unmeasuredBound")}
                    value={t("campaignDetail.budget.unmeasuredBoundValue", {
                      amount: formatCostMicros(unmeasuredBound),
                    })}
                  />
                </dl>
                {authorization.schemaVersion === 2 && authorization.policy && (
                  <section
                    aria-label={t("campaignDetail.budget.operationsAria")}
                  >
                    <h3>{t("campaignDetail.budget.operationsTitle")}</h3>
                    <dl className="detail">
                      <BudgetRow
                        label={t("campaignDetail.budget.modelRequestTimeout")}
                        value={t("campaignDetail.budget.seconds", {
                          count:
                            authorization.policy.operations
                              .modelRequestTimeoutSecs,
                        })}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.toolCallTimeout")}
                        value={t("campaignDetail.budget.seconds", {
                          count:
                            authorization.policy.operations.toolCallTimeoutSecs,
                        })}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.maxRequestBytes")}
                        value={t("campaignDetail.budget.bytes", {
                          count:
                            authorization.policy.operations.maxRequestBytes.toLocaleString(),
                        })}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.maxResponseBytes")}
                        value={t("campaignDetail.budget.bytes", {
                          count:
                            authorization.policy.operations.maxResponseBytes.toLocaleString(),
                        })}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.maxGenerationTokens")}
                        value={authorization.policy.operations.maxGenerationTokens.toLocaleString()}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.maxInFlight")}
                        value={String(
                          authorization.policy.operations.maxInFlightOperations,
                        )}
                      />
                    </dl>
                    <h3>{t("campaignDetail.budget.progressTitle")}</h3>
                    <dl className="detail">
                      <BudgetRow
                        label={t("campaignDetail.budget.diagnoseAfter")}
                        value={String(
                          authorization.policy.progress
                            .diagnoseAfterStagnantCheckpoints,
                        )}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.escalateAfter")}
                        value={String(
                          authorization.policy.progress
                            .escalateAfterUnsuccessfulReplans,
                        )}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.observationTimeout")}
                        value={t("campaignDetail.budget.seconds", {
                          count:
                            authorization.policy.progress
                              .observationTimeoutSecs,
                        })}
                      />
                    </dl>
                  </section>
                )}
                {authorization.schemaVersion === 2 && !authorization.policy && (
                  <p className="warning">
                    {t("campaignDetail.budget.missingPolicy")}
                  </p>
                )}
                {legacyCaps && (
                  <section aria-label={t("campaignDetail.budget.legacyAria")}>
                    <h3>{t("campaignDetail.budget.legacyTitle")}</h3>
                    <p className="muted">
                      {t("campaignDetail.budget.legacyHint")}
                    </p>
                    <dl className="detail">
                      <BudgetRow
                        label={t("campaignDetail.budget.attempts")}
                        value={String(legacyCaps.maxAttempts)}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.sessions")}
                        value={String(legacyCaps.maxSessions)}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.legacyTurns")}
                        value={String(legacyCaps.maxTurns)}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.legacyInputTokens")}
                        value={legacyCaps.maxInputTokens.toLocaleString()}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.legacyOutputTokens")}
                        value={legacyCaps.maxOutputTokens.toLocaleString()}
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.legacyCost")}
                        value={
                          legacyCaps.maxCostMicros > 0
                            ? formatCostMicros(legacyCaps.maxCostMicros)
                            : t("common.notSet")
                        }
                      />
                      <BudgetRow
                        label={t("campaignDetail.budget.deadline")}
                        value={
                          legacyCaps.deadlineSecs > 0
                            ? `${legacyCaps.deadlineSecs}s`
                            : t("common.notSet")
                        }
                      />
                    </dl>
                  </section>
                )}
                <p className="muted">{t("campaignDetail.budget.note")}</p>
                {overBudget.length > 0 && (
                  <p className="warning">
                    {t("campaignDetail.budget.overBudget", {
                      items: overBudget.join(", "),
                    })}
                  </p>
                )}
              </>
            ) : (
              <p className="muted">
                {t("campaignDetail.budget.noAuthorization")}
              </p>
            )}
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>{t("campaignDetail.budget.leadReservationsTitle")}</h2>
              <span className="muted">
                {t("campaignDetail.budget.leadReservationsHint")}
              </span>
            </div>
            {!lead || lead.model_calls.length === 0 ? (
              <EmptyState
                title={t("campaignDetail.budget.noLeadCallsTitle")}
                description={t("campaignDetail.budget.noLeadCallsDescription")}
              />
            ) : (
              <div className="table-responsive">
                <table>
                  <thead>
                    <tr>
                      <th>{t("campaignDetail.budget.table.reservation")}</th>
                      <th>{t("campaignDetail.budget.table.role")}</th>
                      <th>{t("campaignDetail.budget.table.route")}</th>
                      <th>{t("campaignDetail.budget.table.state")}</th>
                      <th>{t("campaignDetail.budget.table.usage")}</th>
                      <th>{t("campaignDetail.budget.table.updated")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {lead.model_calls.map((call) => (
                      <tr key={call.reservation_id}>
                        <td
                          data-label={t(
                            "campaignDetail.budget.table.reservation",
                          )}
                        >
                          <code>{shortId(call.reservation_id)}</code>
                        </td>
                        <td data-label={t("campaignDetail.budget.table.role")}>
                          {call.role}
                        </td>
                        <td data-label={t("campaignDetail.budget.table.route")}>
                          {call.provider}/{call.model}
                        </td>
                        <td data-label={t("campaignDetail.budget.table.state")}>
                          <StatusBadge status={call.state} />
                        </td>
                        <td data-label={t("campaignDetail.budget.table.usage")}>
                          {call.usage_known
                            ? t("campaignDetail.budget.callUsage", {
                                tokens: call.total_tokens.toLocaleString(),
                                cost: formatCostMicros(call.cost_micros),
                              })
                            : "UNMEASURED"}
                        </td>
                        <td
                          className="muted"
                          data-label={t("campaignDetail.budget.table.updated")}
                        >
                          {formatTimestamp(call.updated_at)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </Panel>
        </TabPanel>
      )}

      {tab === "events" && (
        <TabPanel id="events">
          <Panel>
            <div className="section-heading">
              <h2>{t("campaignDetail.events.cancelTitle")}</h2>
              <StatusBadge status={campaign.status} />
            </div>
            <p>
              {t("campaignDetail.events.campaignState")}{" "}
              <StatusBadge status={campaign.status} />
            </p>
            {cancelEvent ? (
              <p>
                <strong>{cancelEvent.kind}</strong> {cancelEvent.detail} ·{" "}
                <span className="muted">
                  {formatTimestamp(cancelEvent.recorded_at)}
                </span>
              </p>
            ) : (
              <p className="muted">{t("campaignDetail.events.noCancel")}</p>
            )}
            {leadBlocked && lead && (
              <p className="warning">
                {t("campaignDetail.events.leadBlocked", {
                  id: shortId(lead.request.id),
                  reason: lead.request.failure_reason ?? "",
                })}
              </p>
            )}
            {leadFailed && lead && (
              <p className="error">
                {t("campaignDetail.events.leadFailed", {
                  id: shortId(lead.request.id),
                  reason:
                    lead.request.failure_reason ??
                    t("campaignDetail.events.noFailureReason"),
                })}
              </p>
            )}
            <p>
              {t("campaignDetail.events.summary", {
                unresolved: unresolvedSessions.length,
                unconfirmed: unconfirmedTermination.length,
              })}
            </p>
            {unconfirmedTermination.length > 0 && (
              <p className="warning">
                {t("campaignDetail.events.terminationWarning", {
                  ids: unconfirmedTermination
                    .map((session) => shortId(session.session_record_id))
                    .join(", "),
                })}
              </p>
            )}
            {campaign.status !== "CANCELLED" &&
              unresolvedSessions.length === 0 && (
                <p className="muted">
                  {t("campaignDetail.events.noUnresolved")}
                </p>
              )}
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>{t("campaignDetail.events.title")}</h2>
              <span className="muted">
                {t("campaignDetail.events.count", {
                  count: detail.events.length,
                })}
              </span>
            </div>
            {detail.events.length === 0 ? (
              <EmptyState
                title={t("campaignDetail.events.emptyTitle")}
                description={t("campaignDetail.events.emptyDescription")}
              />
            ) : (
              <ul className="events">
                {detail.events
                  .slice()
                  .reverse()
                  .map((event) => (
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
        </TabPanel>
      )}
    </div>
  );
}
