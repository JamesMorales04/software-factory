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
import { ParticipantSessionList } from "../components/office/ParticipantSessionList";
import { StatusBadge } from "../components/office/StatusBadge";
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
import {
  formatCostMicros,
  formatObservableLimit,
  formatTimestamp,
  shortId,
} from "../format";

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

function summarizeCounts(counts: Record<string, number>): string {
  const entries = Object.entries(counts);
  if (entries.length === 0) return "none";
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
): string {
  if (candidateIds.length === 0) return "waiting for a Candidate";
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
  return `${recorded}/${candidateIds.length} recorded${detail ? ` · ${detail}` : ""}`;
}

function executionModeLabel(mode: string): string {
  if (mode === "FIXTURE") return "FIXTURE (deterministic, no provider)";
  if (mode === "TRUSTED_LOCAL_PROVIDER")
    return "TRUSTED_LOCAL_PROVIDER (not isolated)";
  return mode;
}

function summarizeGates(
  candidateIds: string[],
  gates: Record<string, Gate | null>,
): string {
  if (candidateIds.length === 0) return "waiting for a Candidate";
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
  return `${decided}/${candidateIds.length} decided${detail ? ` · ${detail}` : " · PENDING"}`;
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
  const { campaignId = "" } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const tab = resolveTab(searchParams.get("tab"));
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
              : "Failed to load Campaign.",
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
  }, [load]);

  async function handleCancelled(campaign: Campaign) {
    setNotice(
      `Campaign ${shortId(campaign.id)} is ${campaign.status}. Core records the cancellation; provider termination can remain unconfirmed.`,
    );
    try {
      await load();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Failed to refresh Campaign after cancellation.",
      );
    }
  }

  if (error && !loaded) {
    return (
      <ErrorState
        message={error}
        action={<Button onClick={() => window.location.reload()}>Retry</Button>}
      />
    );
  }
  if (!loaded) return <LoadingState label="Loading Campaign…" />;

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
  const overBudget: string[] = [];
  if (authorization) {
    if (
      authorization.max_sessions > 0 &&
      sessions.length > authorization.max_sessions
    )
      overBudget.push("sessions");
    if (authorization.max_turns > 0 && turnRecords > authorization.max_turns)
      overBudget.push("turns");
    if (
      authorization.max_input_tokens > 0 &&
      totalInput > authorization.max_input_tokens
    )
      overBudget.push("input tokens");
    if (
      authorization.max_output_tokens > 0 &&
      totalOutput > authorization.max_output_tokens
    )
      overBudget.push("output tokens");
    if (
      authorization.max_cost_micros > 0 &&
      totalCost > authorization.max_cost_micros
    )
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
    attention.push("Campaign is awaiting authorization");
  }
  if (leadBlocked) attention.push("Lead request is BLOCKED");
  if (leadFailed) attention.push("Lead request is FAILED");
  if (detail.runs.some((run) => ["FAILED", "REJECTED"].includes(run.status))) {
    attention.push("A Run failed or was rejected");
  }
  if (
    candidateIds.some((id) =>
      ["REPAIR_REQUIRED", "BLOCKED", "HUMAN_REQUIRED", "NOT_TESTED"].includes(
        gates[id]?.outcome ?? "",
      ),
    )
  ) {
    attention.push("At least one Quality Gate needs review");
  }
  if (unresolvedSessions.length > 0) {
    attention.push("Provider session cleanup is unresolved");
  }
  if (unconfirmedTermination.length > 0) {
    attention.push("Provider termination is unconfirmed");
  }
  if (overBudget.length > 0) {
    attention.push(
      `Measured usage exceeds the authorization for ${overBudget.join(", ")}`,
    );
  }

  const workflow = [
    {
      stage: "Project",
      reference: project ? shortId(project.id) : "not linked",
      state: project ? project.repository : "no Project for this Action",
    },
    {
      stage: "Action",
      reference: action ? shortId(action.id) : "not found",
      state: action
        ? `${action.status} · risk ${action.risk}`
        : "Action not returned by Core",
    },
    {
      stage: "Lead request",
      reference: lead ? shortId(lead.request.id) : "not linked",
      state: lead ? lead.request.state : "no Lead request for this Campaign",
    },
    {
      stage: "Assessment",
      reference: lead?.assessment ? shortId(lead.assessment.id) : "missing",
      state: lead?.assessment
        ? `${lead.assessment.mechanism} mechanism · ${formatTimestamp(lead.assessment.created_at)}`
        : "no assessment recorded",
    },
    {
      stage: "Plan",
      reference: lead?.plan ? shortId(lead.plan.plan_hash, 12) : "not frozen",
      state: lead?.plan
        ? `frozen · config ${shortId(lead.plan.config_hash, 12)}`
        : "no frozen plan recorded",
    },
    {
      stage: "Campaign",
      reference: shortId(campaign.id),
      state: campaign.status,
    },
    {
      stage: "WorkGraph",
      reference: `${detail.tasks.length} unit${detail.tasks.length === 1 ? "" : "s"}`,
      state: summarizeCounts(taskCounts),
    },
    {
      stage: "Runs",
      reference: `${detail.runs.length}`,
      state: summarizeCounts(runCounts),
    },
    {
      stage: "Attempts",
      reference: `${detail.attempts.length}`,
      state: summarizeCounts(attemptCounts),
    },
    {
      stage: "Candidates",
      reference: `${detail.candidates.length}`,
      state:
        detail.candidates.length === 0
          ? "none recorded"
          : `${pendingGates} gate${pendingGates === 1 ? "" : "s"} pending`,
    },
    {
      stage: "Verifier",
      reference: summarizeReceipts(candidateIds, receipts, "verifier"),
      state: "evidence as recorded by Core",
    },
    {
      stage: "Reviewer",
      reference: summarizeReceipts(candidateIds, receipts, "reviewer"),
      state: "missing receipts remain pending",
    },
    {
      stage: "Quality Gate",
      reference: summarizeGates(candidateIds, gates),
      state: "Core-owned deterministic decision",
    },
  ];

  const tabs: TabItem[] = [
    { id: "overview", label: "Overview" },
    { id: "workgraph", label: "WorkGraph", count: detail.tasks.length },
    {
      id: "execution",
      label: "Execution",
      count: detail.runs.length + detail.attempts.length,
    },
    {
      id: "candidates",
      label: "Candidates",
      count: detail.candidates.length,
    },
    {
      id: "evidence",
      label: "Evidence",
      count: candidateIds.reduce(
        (sum, id) => sum + (receipts[id]?.length ?? 0),
        0,
      ),
    },
    {
      id: "gate",
      label: "Quality Gate",
      count: candidateIds.filter((id) => gates[id]).length,
    },
    { id: "budget", label: "Budget & usage" },
    { id: "events", label: "Events", count: detail.events.length },
  ];

  return (
    <div className="campaign-workspace">
      <PageHeader
        title={`Campaign ${shortId(campaign.id)}`}
        description={
          action?.objective ?? "The frozen Action objective is not available."
        }
        meta={
          <>
            <StatusBadge status={campaign.status} />
            <span>
              Action <code>{shortId(campaign.action_id)}</code>
            </span>
            <span>
              {authorization
                ? executionModeLabel(authorization.execution_mode)
                : "Authorization not recorded"}
            </span>
            <span>
              Latest Gate:{" "}
              {latestGate ? (
                <StatusBadge status={latestGate.outcome} />
              ) : (
                "pending"
              )}
            </span>
            <span>
              Fetched {lastLoadedAt ? formatTimestamp(lastLoadedAt) : "—"}
            </span>
          </>
        }
        actions={
          <>
            <CancelCampaign campaign={campaign} onCancelled={handleCancelled} />
            <Link className={buttonClass("quiet", "sm")} to="/campaigns">
              Back to Campaigns
            </Link>
          </>
        }
      />
      {error && (
        <div className="ui-callout ui-callout--danger" role="alert">
          {error} The displayed snapshot may be stale.
        </div>
      )}
      {notice && <p className="notice">{notice}</p>}

      <Panel className="campaign-summary">
        <div className="campaign-summary-head">
          <h2>Operating picture</h2>
          {attention.length === 0 ? (
            <span className="muted">No operator attention signal recorded</span>
          ) : (
            <StatusBadge
              status="HUMAN_REQUIRED"
              label={`${attention.length} attention signal${attention.length === 1 ? "" : "s"}`}
            />
          )}
        </div>
        <dl className="overview-facts">
          <div>
            <dt>Project</dt>
            <dd>
              {project ? (
                <Link to="/projects">{shortId(project.id)}</Link>
              ) : (
                "not linked"
              )}
            </dd>
          </div>
          <div>
            <dt>Action</dt>
            <dd>
              <Link to={`/actions/${campaign.action_id}`}>
                {shortId(campaign.action_id)}
              </Link>
            </dd>
          </div>
          <div>
            <dt>Frozen plan</dt>
            <dd>
              {lead?.plan ? shortId(lead.plan.plan_hash, 12) : "not recorded"}
            </dd>
          </div>
          <div>
            <dt>Authorization</dt>
            <dd>{authorization ? authorization.risk_class : "not recorded"}</dd>
          </div>
          <div>
            <dt>Latest Run</dt>
            <dd>
              {latestRun ? (
                <>
                  <code>{shortId(latestRun.id)}</code>{" "}
                  <StatusBadge status={latestRun.status} />
                </>
              ) : (
                "none"
              )}
            </dd>
          </div>
          <div>
            <dt>Latest Attempt</dt>
            <dd>
              {latestAttempt ? (
                <>
                  <code>{shortId(latestAttempt.id)}</code>{" "}
                  <StatusBadge status={latestAttempt.status} />
                </>
              ) : (
                "none"
              )}
            </dd>
          </div>
          <div>
            <dt>Latest Candidate</dt>
            <dd>{latestCandidate ? shortId(latestCandidate.id) : "none"}</dd>
          </div>
          <div>
            <dt>Latest Gate</dt>
            <dd>
              {latestGate ? (
                <StatusBadge status={latestGate.outcome} />
              ) : (
                "pending"
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
        label="Campaign sections"
      />

      {tab === "overview" && (
        <TabPanel id="overview">
          <Panel>
            <div className="section-heading">
              <h2>Lifecycle projection</h2>
              <span className="muted">
                Campaign authorization, Run execution and Gate acceptance stay
                separate
              </span>
            </div>
            <table data-testid="workflow-projection">
              <thead>
                <tr>
                  <th>Stage</th>
                  <th>Reference</th>
                  <th>Core state</th>
                </tr>
              </thead>
              <tbody>
                {workflow.map((row) => (
                  <tr key={row.stage}>
                    <th scope="row">{row.stage}</th>
                    <td>{row.reference}</td>
                    <td>{row.state}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {lead && lead.model_calls.length > 0 && (
              <div data-testid="lead-model-call-provenance">
                <h3>Lead model calls</h3>
                <p className="muted">
                  {lead.model_calls
                    .map(
                      (call) =>
                        `${call.provider}/${call.model} · reservation ${shortId(call.reservation_id)} · ${call.state} · ${
                          call.usage_known
                            ? `${call.input_tokens.toLocaleString()} in / ${call.output_tokens.toLocaleString()} out / ${call.total_tokens.toLocaleString()} total / ${formatCostMicros(call.cost_micros)}`
                            : "usage UNMEASURED"
                        }`,
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
              <h2>WorkGraph ({detail.tasks.length} units)</h2>
              <span className="muted">{summarizeCounts(taskCounts)}</span>
            </div>
            {detail.tasks.length === 0 ? (
              <EmptyState
                title="No WorkGraph units"
                description="No Lead WorkGraph task is recorded for this Campaign."
              />
            ) : (
              <table>
                <thead>
                  <tr>
                    <th>#</th>
                    <th>Unit</th>
                    <th>Status</th>
                    <th>Depends on</th>
                    <th>Run</th>
                    <th>Attempt</th>
                    <th>Candidate</th>
                  </tr>
                </thead>
                <tbody>
                  {detail.tasks.map((task) => (
                    <tr key={task.task_id}>
                      <td>{task.sequence + 1}</td>
                      <td>
                        <code>{task.unit_id}</code>
                      </td>
                      <td>
                        <StatusBadge status={task.status} />
                      </td>
                      <td>{task.depends_on.join(", ") || "none"}</td>
                      <td>
                        <code>{task.run_id ? shortId(task.run_id) : "—"}</code>
                      </td>
                      <td>
                        <code>
                          {task.attempt_id ? shortId(task.attempt_id) : "—"}
                        </code>
                      </td>
                      <td>
                        {task.candidate_id ? (
                          <>
                            <code>{shortId(task.candidate_id)}</code>
                            {task.candidate_commit && (
                              <div className="muted">
                                commit {shortId(task.candidate_commit, 12)}
                              </div>
                            )}
                          </>
                        ) : (
                          "—"
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </Panel>
        </TabPanel>
      )}

      {tab === "execution" && (
        <TabPanel id="execution">
          <Panel>
            <div className="section-heading">
              <h2>Runs and attempts</h2>
              <span className="muted">
                {summarizeCounts(runCounts)} · {summarizeCounts(attemptCounts)}
              </span>
            </div>
            {detail.runs.length === 0 ? (
              <EmptyState
                title="No Runs"
                description="This Campaign has not queued a Run."
              />
            ) : (
              <>
                <table>
                  <thead>
                    <tr>
                      <th>Run</th>
                      <th>Status</th>
                      <th>Attempts</th>
                    </tr>
                  </thead>
                  <tbody>
                    {detail.runs.map((run) => (
                      <tr key={run.id}>
                        <td>
                          <code>{shortId(run.id)}</code>
                        </td>
                        <td>
                          <StatusBadge status={run.status} />
                        </td>
                        <td>
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
                <h3>Attempts ({detail.attempts.length})</h3>
                {detail.attempts.length === 0 ? (
                  <p className="muted">No Attempts.</p>
                ) : (
                  <table>
                    <thead>
                      <tr>
                        <th>Attempt</th>
                        <th>Run</th>
                        <th>Status</th>
                        <th>Created</th>
                      </tr>
                    </thead>
                    <tbody>
                      {detail.attempts.map((attempt) => (
                        <tr key={attempt.id}>
                          <td>
                            <code>{shortId(attempt.id)}</code>
                          </td>
                          <td>
                            <code>{shortId(attempt.run_id)}</code>
                          </td>
                          <td>
                            <StatusBadge status={attempt.status} />
                          </td>
                          <td className="muted">
                            {formatTimestamp(attempt.created_at)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </>
            )}
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>Participant sessions</h2>
              <span className="muted">
                Cleanup is evidence recorded by the Worker, not an isolation
                certificate
              </span>
            </div>
            <ParticipantSessionList
              sessions={sessions}
              emptyTitle="No provider sessions"
              emptyDescription="No provider session is recorded for this Campaign's Attempts."
            />
          </Panel>
        </TabPanel>
      )}

      {tab === "candidates" && (
        <TabPanel id="candidates">
          {detail.candidates.length === 0 ? (
            <EmptyState
              title="No candidates"
              description="A Gate cannot exist before a Candidate and both independent receipts exist."
            />
          ) : (
            detail.candidates.map((candidate) => {
              const gate = gates[candidate.id] ?? null;
              return (
                <Panel key={candidate.id} data-testid="candidate-panel">
                  <div className="session-card-head">
                    <h2>
                      Candidate <code>{shortId(candidate.id)}</code>
                    </h2>
                    {gate ? (
                      <StatusBadge status={gate.outcome} />
                    ) : (
                      <StatusBadge status="PENDING" label="GATE PENDING" />
                    )}
                  </div>
                  <CandidateIdentity candidate={candidate} />
                  <CandidateDiff
                    campaignId={campaign.id}
                    candidateId={candidate.id}
                  />
                  <div className="campaign-actions">
                    <Button size="sm" onClick={() => selectTab("evidence")}>
                      View evidence
                    </Button>
                    <Button size="sm" onClick={() => selectTab("gate")}>
                      View Gate
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
              title="No evidence"
              description="Verifier and Reviewer receipts appear after a Candidate is recorded."
            />
          ) : (
            detail.candidates.map((candidate) => {
              const candidateReceipts = receipts[candidate.id] ?? [];
              return (
                <Panel key={candidate.id}>
                  <div className="section-heading">
                    <h2>
                      Candidate <code>{shortId(candidate.id)}</code>
                    </h2>
                    <span className="muted">
                      Verifier and Reviewer are independent
                    </span>
                  </div>
                  <ReceiptEvidenceTable receipts={candidateReceipts} />
                  <ReceiptModelProvenance receipts={candidateReceipts} />
                  <h3>Review findings</h3>
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
              title="No Gate decisions"
              description="A Gate decision requires a Candidate and both independent receipts."
            />
          ) : (
            detail.candidates.map((candidate) => (
              <Panel key={candidate.id}>
                <div className="section-heading">
                  <h2>
                    Candidate <code>{shortId(candidate.id)}</code>
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
              <h2>Authorization, budget and observed usage</h2>
              <span className="muted">
                Measured, unmeasured and authorized are separate facts
              </span>
            </div>
            {authorization ? (
              <>
                <p>
                  {authorization.authorization_id} · {authorization.risk_class}{" "}
                  · {executionModeLabel(authorization.execution_mode)} · actor{" "}
                  {authorization.actor}
                </p>
                <p className="muted">{authorization.rationale}</p>
                <dl className="detail">
                  <BudgetRow
                    label="Execution"
                    value={executionModeLabel(authorization.execution_mode)}
                  />
                  <BudgetRow
                    label="Route"
                    value={
                      authorization.execution_mode === "FIXTURE"
                        ? authorization.provider || authorization.model
                          ? `${authorization.provider}/${authorization.model} (${authorization.provider_mode || "mode not set"}; frozen plan route, not executed)`
                          : "fixture execution (no provider route)"
                        : authorization.provider || authorization.model
                          ? `${authorization.provider}/${authorization.model} (${authorization.provider_mode || "mode not set"})`
                          : "provider execution without a recorded route"
                    }
                  />
                  <BudgetRow
                    label="Delegation"
                    value={authorization.delegation_id || "not set"}
                  />
                  <BudgetRow
                    label="Authorized paths"
                    value={authorization.authorized_paths.join(", ") || "none"}
                  />
                  <BudgetRow
                    label="Allowed tools"
                    value={authorization.allowed_tools.join(", ") || "none"}
                  />
                  <BudgetRow
                    label="Attempts"
                    value={`${detail.attempts.length} observed / ${formatObservableLimit(authorization.max_attempts)} authorized`}
                  />
                  <BudgetRow
                    label="Sessions"
                    value={`${sessions.length} observed / ${formatObservableLimit(authorization.max_sessions)} authorized`}
                  />
                  <BudgetRow
                    label="Turn records"
                    value={`${turnRecords} with turn id / ${formatObservableLimit(authorization.max_turns)} authorized`}
                  />
                  <BudgetRow
                    label="Input tokens"
                    value={`${totalInput.toLocaleString()} measured / ${formatObservableLimit(authorization.max_input_tokens)} authorized`}
                  />
                  <BudgetRow
                    label="Output tokens"
                    value={`${totalOutput.toLocaleString()} measured / ${formatObservableLimit(authorization.max_output_tokens)} authorized`}
                  />
                  <BudgetRow
                    label="Cost"
                    value={`${formatCostMicros(totalCost)} measured / ${
                      authorization.max_cost_micros > 0
                        ? formatCostMicros(authorization.max_cost_micros)
                        : "not set"
                    } authorized`}
                  />
                  <BudgetRow
                    label="Unmeasured sessions"
                    value={`${unmeasuredSessions.length} of ${sessions.length}`}
                  />
                  <BudgetRow
                    label="Session unmeasured bound"
                    value={`${formatCostMicros(unmeasuredBound)} observed-session ceiling, not observed spend`}
                  />
                  <BudgetRow
                    label="Deadline"
                    value={
                      authorization.deadline_secs > 0
                        ? `${authorization.deadline_secs}s`
                        : "not set"
                    }
                  />
                </dl>
                <p className="muted">
                  Measured totals exclude usage-unknown sessions. Unmeasured
                  sessions contribute a bounded reservation amount, never an
                  observed token or cost total. These are Core records, not a
                  billing statement.
                </p>
                {overBudget.length > 0 && (
                  <p className="warning">
                    Measured usage exceeds the recorded authorization for{" "}
                    {overBudget.join(", ")}.
                  </p>
                )}
              </>
            ) : (
              <p className="muted">
                No Campaign authorization is recorded, so no execution budget is
                in force.
              </p>
            )}
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>Lead model-call reservations</h2>
              <span className="muted">
                Unresolved calls are resolved in Runtime or the Decision Inbox
              </span>
            </div>
            {!lead || lead.model_calls.length === 0 ? (
              <EmptyState
                title="No Lead model calls"
                description="No Core model-call reservation is recorded for this Lead request."
              />
            ) : (
              <table>
                <thead>
                  <tr>
                    <th>Reservation</th>
                    <th>Role</th>
                    <th>Route</th>
                    <th>State</th>
                    <th>Usage</th>
                    <th>Updated</th>
                  </tr>
                </thead>
                <tbody>
                  {lead.model_calls.map((call) => (
                    <tr key={call.reservation_id}>
                      <td>
                        <code>{shortId(call.reservation_id)}</code>
                      </td>
                      <td>{call.role}</td>
                      <td>
                        {call.provider}/{call.model}
                      </td>
                      <td>
                        <StatusBadge status={call.state} />
                      </td>
                      <td>
                        {call.usage_known
                          ? `${call.total_tokens.toLocaleString()} tokens / ${formatCostMicros(call.cost_micros)}`
                          : "UNMEASURED"}
                      </td>
                      <td className="muted">
                        {formatTimestamp(call.updated_at)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </Panel>
        </TabPanel>
      )}

      {tab === "events" && (
        <TabPanel id="events">
          <Panel>
            <div className="section-heading">
              <h2>Cancellation and recovery</h2>
              <StatusBadge status={campaign.status} />
            </div>
            <p>
              Campaign state: <StatusBadge status={campaign.status} />
            </p>
            {cancelEvent ? (
              <p>
                <strong>{cancelEvent.kind}</strong> {cancelEvent.detail} ·{" "}
                <span className="muted">
                  {formatTimestamp(cancelEvent.recorded_at)}
                </span>
              </p>
            ) : (
              <p className="muted">No campaign.cancelled event is recorded.</p>
            )}
            {leadBlocked && lead && (
              <p className="warning">
                Lead request <code>{shortId(lead.request.id)}</code> is BLOCKED:{" "}
                {lead.request.failure_reason}
              </p>
            )}
            {leadFailed && lead && (
              <p className="error">
                Lead request <code>{shortId(lead.request.id)}</code> is FAILED:{" "}
                {lead.request.failure_reason ?? "no failure reason recorded"}
              </p>
            )}
            <p>
              {unresolvedSessions.length} unresolved provider session
              {unresolvedSessions.length === 1 ? "" : "s"} ·{" "}
              {unconfirmedTermination.length} cancellation request
              {unconfirmedTermination.length === 1 ? "" : "s"} without a cleanup
              receipt.
            </p>
            {unconfirmedTermination.length > 0 && (
              <p className="warning">
                External provider termination is unconfirmed for{" "}
                {unconfirmedTermination
                  .map((session) => shortId(session.session_record_id))
                  .join(", ")}
                . Core records the request, not proof that the provider stopped.
              </p>
            )}
            {campaign.status !== "CANCELLED" &&
              unresolvedSessions.length === 0 && (
                <p className="muted">
                  No unresolved provider session is recorded for this Campaign.
                </p>
              )}
          </Panel>
          <Panel>
            <div className="section-heading">
              <h2>Events</h2>
              <span className="muted">
                {detail.events.length} Core event
                {detail.events.length === 1 ? "" : "s"}
              </span>
            </div>
            {detail.events.length === 0 ? (
              <EmptyState
                title="No Campaign events"
                description="Core records an event when Campaign or related work changes."
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
