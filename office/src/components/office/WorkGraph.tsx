import { type KeyboardEvent, useId, useRef } from "react";
import { Link } from "react-router-dom";
import type {
  Gate,
  LeadTask,
  ProviderSession,
  QualityReceipt,
} from "../../coreApi";
import { formatTimestamp, shortId } from "../../format";
import { Button } from "../ui/Button";
import { cn } from "../ui/cn";
import { EmptyState, Panel } from "../ui/Surface";
import { ParticipantSessionList } from "./ParticipantSessionList";
import { StatusBadge } from "./StatusBadge";
import {
  buildWorkGraphLayout,
  presentationExplanation,
  presentationLabel,
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
  const node = layout.nodes.find((item) => item.task.unit_id === task.unit_id);
  const session =
    sessions.find((item) => item.attempt_id === task.attempt_id) ?? null;
  const verifier = receipts.find((receipt) => receipt.role === "verifier");
  const reviewer = receipts.find((receipt) => receipt.role === "reviewer");
  const unitIssues = issuesForUnit(layout.issues, task.unit_id);
  const state = node?.state ?? "UNKNOWN";
  const missingEvidence: string[] = [];
  if (task.status === "SUCCEEDED" && !task.candidate_id) {
    missingEvidence.push(
      "The WorkUnit is recorded SUCCEEDED but no Candidate is bound.",
    );
  }
  if (task.status === "SUCCEEDED" && task.candidate_id && !gate) {
    missingEvidence.push(
      "No Quality Gate decision is recorded for the bound Candidate.",
    );
  }
  if (task.candidate_id && !verifier) {
    missingEvidence.push("No verifier receipt is recorded.");
  }
  if (task.candidate_id && !reviewer) {
    missingEvidence.push("No reviewer receipt is recorded.");
  }
  if (task.status === "RUNNING" && !task.attempt_id) {
    missingEvidence.push(
      "The WorkUnit is recorded RUNNING but no Attempt is bound.",
    );
  }

  return (
    <Panel
      className="workgraph-inspector"
      as="aside"
      data-testid="workgraph-inspector"
    >
      <div className="section-heading">
        <h3>
          WorkUnit <code>{task.unit_id}</code>
        </h3>
        <Button size="sm" variant="quiet" onClick={onClose}>
          Close
        </Button>
      </div>
      <div className="workgraph-inspector-badges">
        <StatusBadge status={task.status} />
        <StatusBadge status={state} label={presentationLabel(state)} />
      </div>
      <p className="muted">{presentationExplanation(state)}</p>

      <dl className="agent-facts">
        <div>
          <dt>Frozen sequence</dt>
          <dd>{task.sequence}</dd>
        </div>
        <div>
          <dt>Dependencies</dt>
          <dd>
            {task.depends_on.length > 0 ? task.depends_on.join(", ") : "none"}
          </dd>
        </div>
        <div>
          <dt>Base commit</dt>
          <dd>
            <code>{task.base_commit}</code>
          </dd>
        </div>
        <div>
          <dt>Updated</dt>
          <dd>{formatTimestamp(task.updated_at)}</dd>
        </div>
        <div>
          <dt>Run</dt>
          <dd>
            {task.run_id ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=execution`}
              >
                <code>{task.run_id}</code>
              </Link>
            ) : (
              "NOT RECORDED"
            )}
          </dd>
        </div>
        <div>
          <dt>Attempt</dt>
          <dd>
            {task.attempt_id ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=execution`}
              >
                <code>{task.attempt_id}</code>
              </Link>
            ) : (
              "NOT RECORDED"
            )}
          </dd>
        </div>
        <div>
          <dt>Candidate</dt>
          <dd>
            {task.candidate_id ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=candidates`}
              >
                <code>{task.candidate_id}</code>
              </Link>
            ) : (
              "NOT RECORDED"
            )}
          </dd>
        </div>
        <div>
          <dt>Candidate commit</dt>
          <dd>
            {task.candidate_commit ? (
              <code>{task.candidate_commit}</code>
            ) : (
              "NOT RECORDED"
            )}
          </dd>
        </div>
        <div>
          <dt>Candidate tree</dt>
          <dd>
            {task.candidate_tree ? (
              <code>{task.candidate_tree}</code>
            ) : (
              "NOT RECORDED"
            )}
          </dd>
        </div>
        <div>
          <dt>Quality Gate</dt>
          <dd>
            {gate ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=gate`}
              >
                <StatusBadge status={gate.outcome} />
              </Link>
            ) : (
              <StatusBadge status="PENDING" label="NO GATE RECORDED" />
            )}
          </dd>
        </div>
        <div>
          <dt>Verifier</dt>
          <dd>
            {verifier ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=evidence`}
              >
                <StatusBadge status={verifier.result} />{" "}
                <code>{shortId(verifier.principal_id, 18)}</code>
              </Link>
            ) : (
              <StatusBadge status="PENDING" label="NO RECEIPT" />
            )}
          </dd>
        </div>
        <div>
          <dt>Reviewer</dt>
          <dd>
            {reviewer ? (
              <Link
                to={`/campaigns/${encodeURIComponent(campaignId)}?tab=evidence`}
              >
                <StatusBadge status={reviewer.result} />{" "}
                <code>{shortId(reviewer.principal_id, 18)}</code>
              </Link>
            ) : (
              <StatusBadge status="PENDING" label="NO RECEIPT" />
            )}
          </dd>
        </div>
      </dl>

      <div className="workgraph-inspector-section">
        <h4>Participant and session</h4>
        {session ? (
          <>
            <p>
              <Link
                to={`/agents/${encodeURIComponent(`implementer:${session.session_record_id}`)}`}
              >
                Open Implementer participant{" "}
                <code>{shortId(session.session_record_id, 18)}</code>
              </Link>
            </p>
            <ParticipantSessionList
              sessions={[session]}
              showAttempt={false}
              emptyTitle="No provider session"
              emptyDescription="No provider session is bound to this Attempt."
            />
          </>
        ) : (
          <p className="muted">
            No provider session is recorded for this WorkUnit&apos;s Attempt.
          </p>
        )}
      </div>

      {(unitIssues.length > 0 || missingEvidence.length > 0) && (
        <div className="callout callout-attention" role="status">
          <strong>Recorded inconsistency or missing evidence</strong>
          <ul>
            {unitIssues.map((issue) => (
              <li key={`${issue.kind}-${issue.message}`}>{issue.message}</li>
            ))}
            {missingEvidence.map((message) => (
              <li key={message}>{message}</li>
            ))}
          </ul>
          <p className="muted">
            The Office surfaces these conditions; it does not repair graph or
            evidence data.
          </p>
        </div>
      )}

      <p className="muted">
        Selecting a unit keeps the exact Campaign, tab and WorkUnit in the URL
        so reload and browser history restore the same inspection context.
      </p>
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
        title="No WorkGraph units"
        description="This Campaign has no recorded WorkGraph unit. Core does not synthesize a plan in the browser."
      />
    );
  }

  return (
    <div className="workgraph">
      {stale && (
        <div className="callout callout-attention" role="status">
          The Campaign projection is stale. The graph below is the last
          authoritative snapshot; it does not imply that execution stopped.
        </div>
      )}
      <div className="workgraph-heading">
        <div>
          <h3>
            Dependency graph ({layout.nodes.length} unit
            {layout.nodes.length === 1 ? "" : "s"})
          </h3>
          <p className="muted">
            Left-to-right position is derived dependency depth, not scheduling
            progress or completion time.
          </p>
        </div>
        <span className="muted">
          {layout.edges.length} recorded dependency edge
          {layout.edges.length === 1 ? "" : "s"}
        </span>
      </div>

      {layout.issues.length > 0 && (
        <div className="callout callout-attention" role="status">
          <strong>Graph inconsistencies</strong>
          <ul>
            {layout.issues.map((issue) => (
              <li key={`${issue.kind}-${issue.message}`}>{issue.message}</li>
            ))}
          </ul>
          <p className="muted">
            Data is displayed as recorded. The table below remains the
            accessible authoritative alternative.
          </p>
        </div>
      )}

      {selectedUnitMissing && (
        <div className="callout" role="status">
          The selected WorkUnit is not part of this Campaign projection. The URL
          is preserved and no node was selected.
        </div>
      )}

      <section
        className="workgraph-scroll"
        aria-label="WorkGraph visual dependency view"
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
                aria-label={`WorkUnit ${node.task.unit_id}, recorded status ${node.task.status}, presentation ${presentationLabel(node.state)}, ${node.task.depends_on.length} dependencies`}
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
                    label={presentationLabel(node.state)}
                  />
                </span>
                <span className="workgraph-node-meta">
                  {node.task.depends_on.length > 0
                    ? `after ${node.task.depends_on.join(", ")}`
                    : "no dependencies"}
                </span>
                <span className="workgraph-node-meta">
                  {node.task.candidate_id
                    ? `candidate ${shortId(node.task.candidate_id, 10)}${
                        gate ? ` · gate ${gate.outcome}` : " · gate pending"
                      }`
                    : "no candidate recorded"}
                </span>
              </button>
            );
          })}
        </div>
      </section>
      <p className="muted workgraph-scroll-note">
        The canvas scrolls horizontally when needed. Every node is a keyboard
        focus target; arrow keys move between nodes and Enter or Space selects.
      </p>

      <div className="workgraph-table-heading">
        <h3>WorkGraph table (accessible alternative)</h3>
        <span className="muted">
          Exact frozen sequence, dependencies and recorded execution identity.
        </span>
      </div>
      <div className="workgraph-table-wrap">
        <table data-testid="workgraph-table">
          <thead>
            <tr>
              <th>#</th>
              <th>Unit</th>
              <th>Recorded status</th>
              <th>Dependency state</th>
              <th>Depends on</th>
              <th>Run / Attempt</th>
              <th>Candidate</th>
              <th>Gate</th>
              <th>Updated</th>
              <th scope="col" aria-label="Inspect" />
            </tr>
          </thead>
          <tbody>
            {layout.nodes.map((node) => {
              const task = node.task;
              const gate = task.candidate_id
                ? (gates[task.candidate_id] ?? null)
                : null;
              const selected = task.unit_id === selectedUnitId;
              return (
                <tr
                  key={task.unit_id}
                  className={cn(selected && "is-selected")}
                  aria-current={selected ? "true" : undefined}
                >
                  <td>{task.sequence + 1}</td>
                  <th scope="row">
                    <code>{task.unit_id}</code>
                  </th>
                  <td>
                    <StatusBadge status={task.status} />
                  </td>
                  <td>
                    <StatusBadge
                      status={node.state}
                      label={presentationLabel(node.state)}
                    />
                  </td>
                  <td>{task.depends_on.join(", ") || "none"}</td>
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
                      Inspect
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
        <p className="warning">
          Selected WorkUnit has an inconsistent dependency record.
        </p>
      )}
    </div>
  );
}
