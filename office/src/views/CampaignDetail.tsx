import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { CancelCampaign } from "../CancelCampaign";
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
  sessionStateClass,
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

export function CampaignDetail() {
  const { campaignId = "" } = useParams();
  const [loaded, setLoaded] = useState<LoadedCampaign | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const alive = useRef(true);

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
    const timer = setInterval(() => void refresh(), 4000);
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

  if (error && !loaded) return <p className="error">{error}</p>;
  if (!loaded) return <p className="muted">Loading Campaign…</p>;

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
  const unmeasuredSessions = sessions.length - measuredSessions.length;
  const totalInput = sessions.reduce(
    (sum, session) => sum + session.input_tokens,
    0,
  );
  const totalOutput = sessions.reduce(
    (sum, session) => sum + session.output_tokens,
    0,
  );
  const totalCost = sessions.reduce(
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
      state: detail.candidates.length === 0 ? "none recorded" : "recorded",
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

  return (
    <div>
      <div className="view-head">
        <h2>Campaign {shortId(campaign.id)}</h2>
        <span className={`pill ${campaign.status.toLowerCase()}`}>
          {campaign.status}
        </span>
      </div>
      <p className="muted">
        Action <code>{shortId(campaign.action_id)}</code> · Created{" "}
        {formatTimestamp(campaign.created_at)}
      </p>
      <div className="row">
        <CancelCampaign campaign={campaign} onCancelled={handleCancelled} />
        <Link to="/campaigns">Back to Campaigns</Link>
      </div>
      {error && <p className="error">{error}</p>}
      {notice && <p className="notice">{notice}</p>}

      <section>
        <h3>Workflow projection</h3>
        <p className="muted">
          Each stage is read from Core. Missing evidence is shown as missing or
          pending, never as accepted.
        </p>
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
            <h4>Lead model calls</h4>
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
      </section>

      <section>
        <h3>Authorization, budget and observed usage</h3>
        {authorization ? (
          <>
            <p>
              {authorization.authorization_id} · {authorization.risk_class} ·{" "}
              {executionModeLabel(authorization.execution_mode)} · actor{" "}
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
                value={`${unmeasuredSessions} of ${sessions.length}`}
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
              Totals are the measured provider-session records returned by Core;
              they are not a billing statement. Unmeasured sessions do not add
              to token or cost totals.
            </p>
            {overBudget.length > 0 && (
              <p className="warning">
                Observed usage exceeds the recorded authorization for{" "}
                {overBudget.join(", ")}.
              </p>
            )}
          </>
        ) : (
          <p className="muted">
            No Campaign authorization is recorded, so no execution budget is in
            force.
          </p>
        )}
      </section>

      <section>
        <h3>WorkGraph ({detail.tasks.length} units)</h3>
        {detail.tasks.length === 0 ? (
          <p className="muted">No Lead WorkGraph tasks.</p>
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
                    <span className={`pill ${task.status.toLowerCase()}`}>
                      {task.status}
                    </span>
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
      </section>

      <section>
        <h3>Runs and attempts</h3>
        {detail.runs.length === 0 ? (
          <p className="muted">No Runs.</p>
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
                      <span className={`pill ${run.status.toLowerCase()}`}>
                        {run.status}
                      </span>
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
            <h4>Attempts ({detail.attempts.length})</h4>
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
                        <span
                          className={`pill ${attempt.status.toLowerCase()}`}
                        >
                          {attempt.status}
                        </span>
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
      </section>

      <section>
        <h3>Candidates, receipts and Quality Gate</h3>
        {detail.candidates.length === 0 ? (
          <p className="muted">
            No candidates. A gate cannot exist before a Candidate and both
            independent receipts exist.
          </p>
        ) : (
          detail.candidates.map((candidate) => {
            const gate = gates[candidate.id] ?? null;
            const candidateReceipts = receipts[candidate.id] ?? [];
            return (
              <section
                className="candidate-panel"
                key={candidate.id}
                data-testid="candidate-panel"
              >
                <div className="session-card-head">
                  <h4>
                    Candidate <code>{shortId(candidate.id)}</code>
                  </h4>
                  {gate ? (
                    <span className={`pill gate-${gate.outcome.toLowerCase()}`}>
                      {gate.outcome}
                    </span>
                  ) : (
                    <span className="pill">GATE PENDING</span>
                  )}
                </div>
                <dl className="detail">
                  <dt>Commit</dt>
                  <dd>
                    <code title={candidate.commit_sha}>
                      {shortId(candidate.commit_sha, 12)}
                    </code>
                  </dd>
                  <dt>Tree</dt>
                  <dd>
                    <code title={candidate.tree_sha}>
                      {shortId(candidate.tree_sha, 12)}
                    </code>
                  </dd>
                  <dt>Attempt</dt>
                  <dd>
                    <code>{shortId(candidate.attempt_id)}</code>
                  </dd>
                  <dt>Implementer</dt>
                  <dd>
                    <code>{candidate.implementer_id}</code>
                  </dd>
                  <dt>Recorded</dt>
                  <dd>{formatTimestamp(candidate.created_at)}</dd>
                </dl>
                <h5>Verifier and reviewer receipts</h5>
                <table data-testid="candidate-receipts">
                  <thead>
                    <tr>
                      <th>Role</th>
                      <th>Result</th>
                      <th>Evidence</th>
                      <th>Principal</th>
                      <th>Received</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(["verifier", "reviewer"] as QualityRole[]).map((role) => {
                      const receipt = candidateReceipts.find(
                        (item) => item.role === role,
                      );
                      if (!receipt) {
                        return (
                          <tr key={role}>
                            <th scope="row">{role}</th>
                            <td>
                              <span className="muted">PENDING</span>
                            </td>
                            <td className="muted">no receipt recorded</td>
                            <td className="muted">—</td>
                            <td className="muted">—</td>
                          </tr>
                        );
                      }
                      return (
                        <tr key={role}>
                          <th scope="row">{role}</th>
                          <td>
                            <span className={`pill check-${receipt.result}`}>
                              {receipt.result}
                            </span>
                          </td>
                          <td>{receipt.evidence}</td>
                          <td>
                            <code>{receipt.principal_id}</code>
                          </td>
                          <td className="muted">
                            {formatTimestamp(receipt.received_at)}
                            <div>
                              receipt <code>{shortId(receipt.receipt_id)}</code>
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
                {candidateReceipts
                  .filter((receipt) => receipt.mechanism === "MODEL")
                  .map((receipt) => (
                    <p
                      className="muted"
                      key={receipt.receipt_id}
                      data-testid="receipt-model-provenance"
                    >
                      {receipt.role} model:{" "}
                      {receipt.model_provider || "provider not recorded"}/
                      {receipt.model_name || "model not recorded"} · reservation{" "}
                      <code>
                        {receipt.model_reservation_id
                          ? shortId(receipt.model_reservation_id)
                          : "not recorded"}
                      </code>{" "}
                      · request{" "}
                      <code>
                        {receipt.model_request_sha256
                          ? shortId(receipt.model_request_sha256, 12)
                          : "not recorded"}
                      </code>{" "}
                      ·{" "}
                      {receipt.usage_known
                        ? `${receipt.input_tokens.toLocaleString()} in / ${receipt.output_tokens.toLocaleString()} out / ${receipt.total_tokens.toLocaleString()} total / ${formatCostMicros(receipt.cost_micros)}`
                        : "usage UNMEASURED"}
                    </p>
                  ))}
                {candidateReceipts.some(
                  (receipt) => receipt.findings.length > 0,
                ) && (
                  <div data-testid="review-findings">
                    <h5>Review findings</h5>
                    {candidateReceipts
                      .filter((receipt) => receipt.findings.length > 0)
                      .map((receipt) => (
                        <div key={receipt.receipt_id}>
                          <p className="muted">
                            {receipt.role} / {receipt.mechanism}
                          </p>
                          <ul className="findings">
                            {receipt.findings.map((finding) => (
                              <li key={finding.id}>
                                <span
                                  className={`pill check-${finding.severity === "blocking" ? "FAIL" : finding.severity === "warning" ? "CHANGES_REQUESTED" : "PENDING"}`}
                                >
                                  {finding.severity}
                                </span>{" "}
                                <strong>{finding.category}</strong>:{" "}
                                {finding.message}
                                {finding.evidence_refs.length > 0 && (
                                  <span className="muted">
                                    {" "}
                                    ({finding.evidence_refs.join(", ")})
                                  </span>
                                )}
                              </li>
                            ))}
                          </ul>
                        </div>
                      ))}
                  </div>
                )}
                <h5>Quality Gate</h5>
                {gate ? (
                  <>
                    <p>
                      <span
                        className={`pill gate-${gate.outcome.toLowerCase()}`}
                      >
                        {gate.outcome}
                      </span>{" "}
                      <span className="muted">
                        decided {formatTimestamp(gate.decided_at)}
                      </span>
                    </p>
                    {gate.reasons.length === 0 ? (
                      <p className="muted">No reasons recorded.</p>
                    ) : (
                      <ul className="findings">
                        {gate.reasons.map((reason) => (
                          <li key={reason}>{reason}</li>
                        ))}
                      </ul>
                    )}
                  </>
                ) : (
                  <p className="warning">
                    PENDING — no Core gate decision is recorded. Missing
                    receipts or non-passing evidence are not approval.
                  </p>
                )}
              </section>
            );
          })
        )}
      </section>

      <section>
        <h3>Provider sessions and cleanup ({sessions.length})</h3>
        {sessions.length === 0 ? (
          <p className="muted">
            No provider sessions recorded for this Campaign's Attempts.
          </p>
        ) : (
          <table>
            <thead>
              <tr>
                <th>Session</th>
                <th>Attempt</th>
                <th>Worker / route</th>
                <th>State</th>
                <th>Usage</th>
                <th>Cleanup</th>
                <th>Updated</th>
              </tr>
            </thead>
            <tbody>
              {sessions.map((session) => (
                <tr key={session.session_record_id}>
                  <td>
                    <code>{shortId(session.session_record_id)}</code>
                    <div className="muted">generation {session.generation}</div>
                  </td>
                  <td>
                    <code>{shortId(session.attempt_id)}</code>
                  </td>
                  <td>
                    {session.provider}/{session.model}
                    <div className="muted">
                      {session.worker_id} · {session.provider_mode}
                    </div>
                  </td>
                  <td>
                    <span className={sessionStateClass(session.state)}>
                      {session.state}
                    </span>
                  </td>
                  <td>
                    {session.usage_known ? (
                      <>
                        {session.input_tokens.toLocaleString()} in /{" "}
                        {session.output_tokens.toLocaleString()} out
                        <div className="muted">
                          {session.total_tokens.toLocaleString()} tokens ·{" "}
                          {formatCostMicros(session.cost_micros)}
                        </div>
                      </>
                    ) : (
                      <span className="muted">UNMEASURED</span>
                    )}
                  </td>
                  <td>
                    {session.cleanup_receipt || (
                      <span className="warning">no cleanup receipt</span>
                    )}
                    {session.error && (
                      <div className="error">{session.error}</div>
                    )}
                  </td>
                  <td className="muted">
                    {formatTimestamp(session.updated_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <section>
        <h3>Cancellation and recovery</h3>
        <p>
          Campaign state:{" "}
          <span className={`pill ${campaign.status.toLowerCase()}`}>
            {campaign.status}
          </span>
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
        {campaign.status !== "CANCELLED" && unresolvedSessions.length === 0 && (
          <p className="muted">
            No unresolved provider session is recorded for this Campaign.
          </p>
        )}
      </section>

      <section>
        <h3>Events</h3>
        {detail.events.length === 0 ? (
          <p className="muted">No Campaign events.</p>
        ) : (
          <ul className="events">
            {detail.events.slice(-20).map((event) => (
              <li key={event.sequence}>
                <span className="muted">
                  {formatTimestamp(event.recorded_at)}
                </span>{" "}
                <strong>{event.kind}</strong> {event.detail}
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
