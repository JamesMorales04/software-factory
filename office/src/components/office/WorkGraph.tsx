import { type KeyboardEvent, useId, useRef } from "react";
import { Link } from "react-router-dom";
import type {
  Gate,
  LeadTask,
  ProviderSession,
  QualityReceipt,
} from "../../coreApi";
import { formatTimestamp, shortId } from "../../format";
import { type MessageKey, useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { cn } from "../ui/cn";
import { EmptyState, Panel } from "../ui/Surface";
import { ParticipantSessionList } from "./ParticipantSessionList";
import { StatusBadge } from "./StatusBadge";
import {
  buildWorkGraphLayout,
  WORKGRAPH_NODE_HEIGHT,
  WORKGRAPH_NODE_WIDTH,
  type WorkGraphIssue,
  type WorkGraphNode,
  type WorkGraphPresentationState,
} from "./workGraphLayout";

const STATE_TONE: Record<WorkGraphPresentationState, string> = {
  READY: "workgraph-state-ready",
  WAITING: "workgraph-state-waiting",
  BLOCKED: "workgraph-state-blocked",
  RUNNING: "workgraph-state-running",
  SUCCEEDED: "workgraph-state-succeeded",
  FAILED: "workgraph-state-failed",
  CANCELLED: "workgraph-state-cancelled",
  UNKNOWN: "workgraph-state-unknown",
  INCONSISTENT: "workgraph-state-inconsistent",
};

const PRESENTATION_LABEL: Record<WorkGraphPresentationState, MessageKey> = {
  READY: "workgraph.presentation.ready",
  WAITING: "workgraph.presentation.waiting",
  BLOCKED: "workgraph.presentation.blocked",
  RUNNING: "workgraph.presentation.running",
  SUCCEEDED: "workgraph.presentation.succeeded",
  FAILED: "workgraph.presentation.failed",
  CANCELLED: "workgraph.presentation.cancelled",
  UNKNOWN: "workgraph.presentation.unknown",
  INCONSISTENT: "workgraph.presentation.inconsistent",
};

const PRESENTATION_EXPLANATION: Record<WorkGraphPresentationState, MessageKey> =
  {
    READY: "workgraph.explanation.ready",
    WAITING: "workgraph.explanation.waiting",
    BLOCKED: "workgraph.explanation.blocked",
    RUNNING: "workgraph.explanation.running",
    SUCCEEDED: "workgraph.explanation.succeeded",
    FAILED: "workgraph.explanation.failed",
    CANCELLED: "workgraph.explanation.cancelled",
    UNKNOWN: "workgraph.explanation.unknown",
    INCONSISTENT: "workgraph.explanation.inconsistent",
  };

function presentationLabel(
  state: WorkGraphPresentationState,
  t: ReturnType<typeof useI18n>["t"],
): string {
  return t(PRESENTATION_LABEL[state]);
}

function presentationExplanation(
  state: WorkGraphPresentationState,
  t: ReturnType<typeof useI18n>["t"],
): string {
  return t(PRESENTATION_EXPLANATION[state]);
}

function issueMessage(
  issue: WorkGraphIssue,
  t: ReturnType<typeof useI18n>["t"],
): string {
  switch (issue.kind) {
    case "MISSING_DEPENDENCY":
      return t("workgraph.issue.missingDependency", {
        unit: issue.unitId ?? "",
        dependency: issue.dependency ?? "",
      });
    case "SELF_DEPENDENCY":
      return t("workgraph.issue.selfDependency", { unit: issue.unitId ?? "" });
    case "CYCLE":
      return t("workgraph.issue.cycle");
    case "DUPLICATE_UNIT":
      return t("workgraph.issue.duplicateUnit", { unit: issue.unitId ?? "" });
    case "DUPLICATE_TASK":
      return t("workgraph.issue.duplicateTask", {
        task: issue.taskId ?? "",
      });
    case "SEQUENCE_ORDER":
      return t("workgraph.issue.sequenceOrder", {
        unit: issue.unitId ?? "",
        sequence: issue.sequence ?? 0,
        dependency: issue.dependency ?? "",
        dependencySequence: issue.dependencySequence ?? 0,
      });
    case "UNKNOWN_STATUS":
      return t("workgraph.issue.unknownStatus", {
        unit: issue.unitId ?? "",
        status: issue.status ?? "",
      });
    default:
      return issue.message;
  }
}

function edgePath(from: WorkGraphNode, to: WorkGraphNode): string {
  const x1 = from.x + WORKGRAPH_NODE_WIDTH;
  const y1 = from.y + WORKGRAPH_NODE_HEIGHT / 2;
  const x2 = to.x;
  const y2 = to.y + WORKGRAPH_NODE_HEIGHT / 2;
  const bend = Math.max(28, Math.abs(x2 - x1) / 2);
  return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`;
}

function issuesForUnit(issues: WorkGraphIssue[], unitId: string) {
  return issues.filter((issue) => issue.unitId === unitId);
}

function WorkUnitInspector({
  task,
  layout,
  campaignId,
  sessions,
  gate,
  receipts,
  onClose,
}: {
  task: LeadTask;
  layout: ReturnType<typeof buildWorkGraphLayout>;
  campaignId: string;
  sessions: ProviderSession[];
  gate: Gate | null;
  receipts: QualityReceipt[];
  onClose: () => void;
}) {
  const { t } = useI18n();
  const node = layout.nodes.find((item) => item.task.unit_id === task.unit_id);
  const session =
    sessions
      .filter((item) => item.attempt_id === task.attempt_id)
      .sort((left, right) => right.generation - left.generation)[0] ?? null;
  const verifier = receipts.find((receipt) => receipt.role === "verifier");
  const reviewer = receipts.find((receipt) => receipt.role === "reviewer");
  const unitIssues = issuesForUnit(layout.issues, task.unit_id);
  const state = node?.state ?? "UNKNOWN";
  const missingEvidence: string[] = [];
  if (task.status === "SUCCEEDED" && !task.candidate_id) {
    missingEvidence.push(t("workgraph.missing.candidateNotBound"));
  }
  if (task.status === "SUCCEEDED" && task.candidate_id && !gate) {
    missingEvidence.push(t("workgraph.missing.gateNotRecorded"));
  }
  if (task.candidate_id && !verifier) {
    missingEvidence.push(t("workgraph.missing.verifierReceipt"));
  }
  if (task.candidate_id && !reviewer) {
    missingEvidence.push(t("workgraph.missing.reviewerReceipt"));
  }
  if (task.status === "RUNNING" && !task.attempt_id) {
    missingEvidence.push(t("workgraph.missing.attemptNotBound"));
  }

  return (
    <Panel
      className="workgraph-inspector"
      as="aside"
      data-testid="workgraph-inspector"
    >
      <div className="section-heading">
        <h3>
          {t("workgraph.inspector.title")} <code>{task.unit_id}</code>
        </h3>
        <Button size="sm" variant="quiet" onClick={onClose}>
          {t("workgraph.inspector.close")}
        </Button>
      </div>
      <div className="workgraph-inspector-badges">
        <StatusBadge status={task.status} />
        <StatusBadge status={state} label={presentationLabel(state, t)} />
      </div>
      <p className="muted">{presentationExplanation(state, t)}</p>

      <dl className="agent-facts">
        <div>
          <dt>{t("workgraph.fact.sequence")}</dt>
          <dd>{task.sequence}</dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.dependencies")}</dt>
          <dd>
            {task.depends_on.length > 0
              ? task.depends_on.join(", ")
              : t("workgraph.none")}
          </dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.baseCommit")}</dt>
          <dd>
            <code>{task.base_commit}</code>
          </dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.updated")}</dt>
          <dd>{formatTimestamp(task.updated_at)}</dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.run")}</dt>
          <dd>
            {task.run_id ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=execution`}
              >
                <code>{task.run_id}</code>
              </Link>
            ) : (
              t("workgraph.notRecorded")
            )}
          </dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.attempt")}</dt>
          <dd>
            {task.attempt_id ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=execution`}
              >
                <code>{task.attempt_id}</code>
              </Link>
            ) : (
              t("workgraph.notRecorded")
            )}
          </dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.candidate")}</dt>
          <dd>
            {task.candidate_id ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=candidates`}
              >
                <code>{task.candidate_id}</code>
              </Link>
            ) : (
              t("workgraph.notRecorded")
            )}
          </dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.candidateCommit")}</dt>
          <dd>
            {task.candidate_commit ? (
              <code>{task.candidate_commit}</code>
            ) : (
              t("workgraph.notRecorded")
            )}
          </dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.candidateTree")}</dt>
          <dd>
            {task.candidate_tree ? (
              <code>{task.candidate_tree}</code>
            ) : (
              t("workgraph.notRecorded")
            )}
          </dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.gate")}</dt>
          <dd>
            {gate ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=gate`}
              >
                <StatusBadge status={gate.outcome} />
              </Link>
            ) : (
              <StatusBadge status="PENDING" label={t("workgraph.noGate")} />
            )}
          </dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.verifier")}</dt>
          <dd>
            {verifier ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=evidence`}
              >
                <StatusBadge status={verifier.result} />{" "}
                <code>{shortId(verifier.principal_id, 18)}</code>
              </Link>
            ) : (
              <StatusBadge status="PENDING" label={t("workgraph.noReceipt")} />
            )}
          </dd>
        </div>
        <div>
          <dt>{t("workgraph.fact.reviewer")}</dt>
          <dd>
            {reviewer ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=evidence`}
              >
                <StatusBadge status={reviewer.result} />{" "}
                <code>{shortId(reviewer.principal_id, 18)}</code>
              </Link>
            ) : (
              <StatusBadge status="PENDING" label={t("workgraph.noReceipt")} />
            )}
          </dd>
        </div>
      </dl>

      <div className="workgraph-inspector-section">
        <h4>{t("workgraph.participant.title")}</h4>
        {session ? (
          <>
            <p>
              <Link
                to={`/agents/${encodeURIComponent(`implementer:${session.session_record_id}`)}`}
              >
                {t("workgraph.participant.open")}{" "}
                <code>{shortId(session.session_record_id, 18)}</code>
              </Link>
            </p>
            <ParticipantSessionList
              sessions={[session]}
              showAttempt={false}
              emptyTitle={t("workgraph.participant.emptyTitle")}
              emptyDescription={t("workgraph.participant.emptyDescription")}
            />
          </>
        ) : (
          <p className="muted">{t("workgraph.participant.none")}</p>
        )}
      </div>

      {(unitIssues.length > 0 || missingEvidence.length > 0) && (
        <div className="callout callout-attention" role="status">
          <strong>{t("workgraph.inconsistency.title")}</strong>
          <ul>
            {unitIssues.map((issue) => (
              <li key={`${issue.kind}-${issue.message}`}>
                {issueMessage(issue, t)}
              </li>
            ))}
            {missingEvidence.map((message) => (
              <li key={message}>{message}</li>
            ))}
          </ul>
          <p className="muted">{t("workgraph.inconsistency.hint")}</p>
        </div>
      )}

      <p className="muted">{t("workgraph.selectionHint")}</p>
    </Panel>
  );
}

export function WorkGraphPanel({
  tasks,
  campaignId,
  selectedUnitId,
  onSelectUnit,
  sessions,
  gates,
  receipts,
  stale = false,
  selectedUnitMissing = false,
}: {
  tasks: LeadTask[];
  campaignId: string;
  selectedUnitId: string | null;
  onSelectUnit: (unitId: string | null) => void;
  sessions: ProviderSession[];
  gates: Record<string, Gate | null>;
  receipts: Record<string, QualityReceipt[]>;
  stale?: boolean;
  selectedUnitMissing?: boolean;
}) {
  const { t } = useI18n();
  const markerId = `workgraph-arrow-${useId().replaceAll(/[^a-zA-Z0-9]/g, "")}`;
  const refs = useRef<Record<string, HTMLButtonElement | null>>({});
  const layout = buildWorkGraphLayout(tasks);
  const selectedTask =
    tasks.find((task) => task.unit_id === selectedUnitId) ?? null;
  const selectedNode = selectedTask
    ? layout.nodes.find((node) => node.task.unit_id === selectedTask.unit_id)
    : null;

  function focusNeighbor(node: WorkGraphNode, key: string) {
    const candidates = layout.nodes
      .filter((candidate) => candidate.task.unit_id !== node.task.unit_id)
      .map((candidate) => {
        const dx =
          candidate.x +
          WORKGRAPH_NODE_WIDTH / 2 -
          (node.x + WORKGRAPH_NODE_WIDTH / 2);
        const dy =
          candidate.y +
          WORKGRAPH_NODE_HEIGHT / 2 -
          (node.y + WORKGRAPH_NODE_HEIGHT / 2);
        return { candidate, dx, dy };
      })
      .filter(({ dx, dy }) => {
        if (key === "ArrowRight") return dx > 0 && Math.abs(dy) <= Math.abs(dx);
        if (key === "ArrowLeft") return dx < 0 && Math.abs(dy) <= Math.abs(dx);
        if (key === "ArrowDown") return dy > 0 && Math.abs(dx) <= Math.abs(dy);
        if (key === "ArrowUp") return dy < 0 && Math.abs(dx) <= Math.abs(dy);
        return false;
      })
      .sort(
        (left, right) =>
          Math.hypot(left.dx, left.dy) - Math.hypot(right.dx, right.dy),
      );
    const next = candidates[0]?.candidate;
    if (next) {
      refs.current[next.task.unit_id]?.focus();
    }
  }

  function onNodeKeyDown(
    event: KeyboardEvent<HTMLButtonElement>,
    node: WorkGraphNode,
  ) {
    if (
      ["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp"].includes(event.key)
    ) {
      event.preventDefault();
      focusNeighbor(node, event.key);
    }
  }

  if (tasks.length === 0) {
    return (
      <EmptyState
        title={t("workgraph.emptyTitle")}
        description={t("workgraph.emptyDescription")}
      />
    );
  }

  return (
    <div className="workgraph">
      {stale && (
        <div className="callout callout-attention" role="status">
          {t("workgraph.stale")}
        </div>
      )}
      <div className="workgraph-heading">
        <div>
          <h3>
            {layout.nodes.length === 1
              ? t("workgraph.headingOne", { count: layout.nodes.length })
              : t("workgraph.heading", { count: layout.nodes.length })}
          </h3>
          <p className="muted">{t("workgraph.headingHint")}</p>
        </div>
        <span className="muted">
          {layout.edges.length === 1
            ? t("workgraph.edgesOne", { count: layout.edges.length })
            : t("workgraph.edges", { count: layout.edges.length })}
        </span>
      </div>

      {layout.issues.length > 0 && (
        <div className="callout callout-attention" role="status">
          <strong>{t("workgraph.issuesTitle")}</strong>
          <ul>
            {layout.issues.map((issue) => (
              <li key={`${issue.kind}-${issue.message}`}>
                {issueMessage(issue, t)}
              </li>
            ))}
          </ul>
          <p className="muted">{t("workgraph.issuesHint")}</p>
        </div>
      )}

      {selectedUnitMissing && (
        <div className="callout" role="status">
          {t("workgraph.selectedMissing")}
        </div>
      )}

      <section
        className="workgraph-scroll"
        aria-label={t("workgraph.canvasAria")}
      >
        <div
          className="workgraph-canvas"
          style={{ width: layout.width, height: layout.height }}
        >
          <svg
            className="workgraph-edges"
            width={layout.width}
            height={layout.height}
            aria-hidden="true"
          >
            <defs>
              <marker
                id={markerId}
                viewBox="0 0 10 10"
                refX="9"
                refY="5"
                markerWidth="7"
                markerHeight="7"
                orient="auto-start-reverse"
              >
                <path d="M 0 0 L 10 5 L 0 10 z" className="workgraph-arrow" />
              </marker>
            </defs>
            {layout.edges.map((edge) => {
              const from = layout.nodes.find(
                (node) => node.task.unit_id === edge.from,
              );
              const to = layout.nodes.find(
                (node) => node.task.unit_id === edge.to,
              );
              if (!from || !to) return null;
              return (
                <path
                  key={`${edge.from}->${edge.to}`}
                  d={edgePath(from, to)}
                  className={cn(
                    "workgraph-edge",
                    ["FAILED", "CANCELLED", "BLOCKED", "INCONSISTENT"].includes(
                      edge.targetState,
                    ) && "workgraph-edge-attention",
                  )}
                  markerEnd={`url(#${markerId})`}
                />
              );
            })}
          </svg>
          {layout.nodes.map((node) => {
            const selected = node.task.unit_id === selectedUnitId;
            const gate = node.task.candidate_id
              ? (gates[node.task.candidate_id] ?? null)
              : null;
            return (
              <button
                key={node.task.unit_id}
                ref={(element) => {
                  refs.current[node.task.unit_id] = element;
                }}
                type="button"
                className={cn(
                  "workgraph-node",
                  STATE_TONE[node.state],
                  selected && "is-selected",
                )}
                style={{
                  left: node.x,
                  top: node.y,
                  width: WORKGRAPH_NODE_WIDTH,
                  height: WORKGRAPH_NODE_HEIGHT,
                }}
                aria-pressed={selected}
                aria-label={t("workgraph.nodeAria", {
                  unit: node.task.unit_id,
                  status: node.task.status,
                  presentation: presentationLabel(node.state, t),
                  count: node.task.depends_on.length,
                })}
                onClick={() => onSelectUnit(node.task.unit_id)}
                onKeyDown={(event) => onNodeKeyDown(event, node)}
                data-testid={`workgraph-node-${node.task.unit_id}`}
              >
                <span className="workgraph-node-top">
                  <span className="workgraph-node-seq">
                    #{node.task.sequence + 1}
                  </span>
                  <StatusBadge status={node.task.status} />
                </span>
                <span className="workgraph-node-unit">
                  <code>{node.task.unit_id}</code>
                </span>
                <span className="workgraph-node-state">
                  <StatusBadge
                    status={node.state}
                    label={presentationLabel(node.state, t)}
                  />
                </span>
                <span className="workgraph-node-meta">
                  {node.task.depends_on.length > 0
                    ? t("workgraph.node.after", {
                        units: node.task.depends_on.join(", "),
                      })
                    : t("workgraph.node.noDependencies")}
                </span>
                <span className="workgraph-node-meta">
                  {node.task.candidate_id
                    ? gate
                      ? t("workgraph.node.candidateGate", {
                          candidate: shortId(node.task.candidate_id, 10),
                          gate: gate.outcome,
                        })
                      : t("workgraph.node.candidatePending", {
                          candidate: shortId(node.task.candidate_id, 10),
                        })
                    : t("workgraph.node.noCandidate")}
                </span>
              </button>
            );
          })}
        </div>
      </section>
      <p className="muted workgraph-scroll-note">{t("workgraph.scrollNote")}</p>

      <div className="workgraph-table-heading">
        <h3>{t("workgraph.table.title")}</h3>
        <span className="muted">{t("workgraph.table.hint")}</span>
      </div>
      <div className="workgraph-table-wrap">
        <table data-testid="workgraph-table">
          <thead>
            <tr>
              <th>#</th>
              <th>{t("workgraph.table.unit")}</th>
              <th>{t("workgraph.table.recordedStatus")}</th>
              <th>{t("workgraph.table.dependencyState")}</th>
              <th>{t("workgraph.table.dependsOn")}</th>
              <th>{t("workgraph.table.runAttempt")}</th>
              <th>{t("workgraph.table.candidate")}</th>
              <th>{t("workgraph.table.gate")}</th>
              <th>{t("workgraph.table.updated")}</th>
              <th scope="col" aria-label={t("workgraph.table.inspect")} />
            </tr>
          </thead>
          <tbody>
            {layout.tableRows.map((row) => {
              const task = row.task;
              const gate = task.candidate_id
                ? (gates[task.candidate_id] ?? null)
                : null;
              const selected = task.unit_id === selectedUnitId;
              const duplicateUnit =
                layout.tableRows.filter(
                  (candidate) => candidate.task.unit_id === task.unit_id,
                ).length > 1;
              return (
                <tr
                  key={task.task_id}
                  className={cn(selected && "is-selected")}
                  aria-current={selected ? "true" : undefined}
                >
                  <td>{task.sequence + 1}</td>
                  <th scope="row">
                    <code>{task.unit_id}</code>
                    {(duplicateUnit || !row.graphNode) && (
                      <div className="muted">
                        {t("workgraph.table.task")}{" "}
                        <code>{shortId(task.task_id, 12)}</code>
                      </div>
                    )}
                  </th>
                  <td>
                    <StatusBadge status={task.status} />
                  </td>
                  <td>
                    <StatusBadge
                      status={row.state}
                      label={presentationLabel(row.state, t)}
                    />
                  </td>
                  <td>{task.depends_on.join(", ") || t("workgraph.none")}</td>
                  <td>
                    {task.run_id ? shortId(task.run_id) : "—"} /{" "}
                    {task.attempt_id ? shortId(task.attempt_id) : "—"}
                  </td>
                  <td>
                    {task.candidate_id ? (
                      <>
                        {shortId(task.candidate_id)}
                        {task.candidate_commit && (
                          <div className="muted">
                            <code>{shortId(task.candidate_commit, 12)}</code>
                          </div>
                        )}
                      </>
                    ) : (
                      "—"
                    )}
                  </td>
                  <td>
                    {gate ? (
                      <StatusBadge status={gate.outcome} />
                    ) : task.candidate_id ? (
                      <StatusBadge status="PENDING" label="PENDING" />
                    ) : (
                      "—"
                    )}
                  </td>
                  <td className="muted">{formatTimestamp(task.updated_at)}</td>
                  <td>
                    <button
                      type="button"
                      className="link-button"
                      aria-pressed={selected}
                      onClick={() => onSelectUnit(task.unit_id)}
                    >
                      {t("workgraph.table.inspect")}
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {selectedTask && (
        <WorkUnitInspector
          task={selectedTask}
          layout={layout}
          campaignId={campaignId}
          sessions={sessions}
          gate={
            selectedTask.candidate_id
              ? (gates[selectedTask.candidate_id] ?? null)
              : null
          }
          receipts={
            selectedTask.candidate_id
              ? (receipts[selectedTask.candidate_id] ?? [])
              : []
          }
          onClose={() => onSelectUnit(null)}
        />
      )}
      {selectedNode && selectedNode.state === "INCONSISTENT" && (
        <p className="warning">{t("workgraph.selectedInconsistent")}</p>
      )}
    </div>
  );
}
