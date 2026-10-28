import type { LeadTask } from "../../coreApi";

export const WORKGRAPH_NODE_WIDTH = 224;
export const WORKGRAPH_NODE_HEIGHT = 96;
export const WORKGRAPH_GAP_X = 72;
export const WORKGRAPH_GAP_Y = 24;
export const WORKGRAPH_PADDING = 24;

export type WorkGraphPresentationState =
  | "READY"
  | "WAITING"
  | "BLOCKED"
  | "RUNNING"
  | "SUCCEEDED"
  | "FAILED"
  | "CANCELLED"
  | "UNKNOWN"
  | "INCONSISTENT";

export interface WorkGraphNode {
  task: LeadTask;
  layer: number;
  row: number;
  x: number;
  y: number;
  state: WorkGraphPresentationState;
}

export interface WorkGraphEdge {
  from: string;
  to: string;
  targetState: WorkGraphPresentationState;
}

export type WorkGraphIssueKind =
  | "MISSING_DEPENDENCY"
  | "SELF_DEPENDENCY"
  | "CYCLE"
  | "DUPLICATE_UNIT"
  | "DUPLICATE_TASK"
  | "SEQUENCE_ORDER"
  | "UNKNOWN_STATUS";

export interface WorkGraphIssue {
  kind: WorkGraphIssueKind;
  message: string;
  unitId?: string;
}

export interface WorkGraphTableRow {
  task: LeadTask;
  state: WorkGraphPresentationState;
  graphNode: boolean;
}

export interface WorkGraphLayout {
  nodes: WorkGraphNode[];
  tableRows: WorkGraphTableRow[];
  edges: WorkGraphEdge[];
  issues: WorkGraphIssue[];
  width: number;
  height: number;
  layerCount: number;
  hasCycle: boolean;
}

const KNOWN_STATUSES = new Set([
  "PENDING",
  "RUNNING",
  "SUCCEEDED",
  "FAILED",
  "CANCELLED",
]);

function terminalFailure(status: string): boolean {
  return status === "FAILED" || status === "CANCELLED";
}

function presentationState(
  task: LeadTask,
  byUnit: Map<string, LeadTask>,
  issues: WorkGraphIssue[],
): WorkGraphPresentationState {
  const status = task.status.toUpperCase();
  if (!KNOWN_STATUSES.has(status)) {
    issues.push({
      kind: "UNKNOWN_STATUS",
      message: `WorkUnit ${task.unit_id} has an unrecognized recorded status ${task.status}.`,
      unitId: task.unit_id,
    });
    return "UNKNOWN";
  }
  if (status === "RUNNING") return "RUNNING";
  if (status === "SUCCEEDED") return "SUCCEEDED";
  if (status === "FAILED") return "FAILED";
  if (status === "CANCELLED") return "CANCELLED";
  const dependencies = task.depends_on
    .map((unit) => byUnit.get(unit))
    .filter((dependency): dependency is LeadTask => dependency !== undefined);
  if (dependencies.length !== task.depends_on.length) {
    return "INCONSISTENT";
  }
  if (dependencies.some((dependency) => terminalFailure(dependency.status))) {
    return "BLOCKED";
  }
  if (
    dependencies.length === 0 ||
    dependencies.every((dependency) => dependency.status === "SUCCEEDED")
  ) {
    return "READY";
  }
  return "WAITING";
}

function detectCycle(
  units: string[],
  dependencies: Map<string, string[]>,
): boolean {
  const state = new Map<string, "visiting" | "done">();
  function visit(unit: string): boolean {
    if (state.get(unit) === "visiting") return true;
    if (state.get(unit) === "done") return false;
    state.set(unit, "visiting");
    for (const dependency of dependencies.get(unit) ?? []) {
      if (visit(dependency)) return true;
    }
    state.set(unit, "done");
    return false;
  }
  return units.some((unit) => visit(unit));
}

export function buildWorkGraphLayout(tasks: LeadTask[]): WorkGraphLayout {
  const issues: WorkGraphIssue[] = [];
  const ordered = [...tasks].sort(
    (left, right) =>
      left.sequence - right.sequence ||
      left.unit_id.localeCompare(right.unit_id),
  );
  const byUnit = new Map<string, LeadTask>();
  const seenTaskIds = new Set<string>();
  for (const task of ordered) {
    if (byUnit.has(task.unit_id)) {
      issues.push({
        kind: "DUPLICATE_UNIT",
        message: `WorkUnit ${task.unit_id} is recorded more than once.`,
        unitId: task.unit_id,
      });
      continue;
    }
    if (seenTaskIds.has(task.task_id)) {
      issues.push({
        kind: "DUPLICATE_TASK",
        message: `Task ${task.task_id} is recorded more than once.`,
        unitId: task.unit_id,
      });
      continue;
    }
    seenTaskIds.add(task.task_id);
    byUnit.set(task.unit_id, task);
  }

  const units = ordered
    .map((task) => task.unit_id)
    .filter(
      (unit, index, all) => all.indexOf(unit) === index && byUnit.has(unit),
    );
  const dependencies = new Map<string, string[]>();
  const edges: WorkGraphEdge[] = [];
  for (const unit of units) {
    const task = byUnit.get(unit);
    if (!task) continue;
    const existing: string[] = [];
    for (const dependency of task.depends_on) {
      if (dependency === unit) {
        issues.push({
          kind: "SELF_DEPENDENCY",
          message: `WorkUnit ${unit} depends on itself.`,
          unitId: unit,
        });
        continue;
      }
      const dependencyTask = byUnit.get(dependency);
      if (!dependencyTask) {
        issues.push({
          kind: "MISSING_DEPENDENCY",
          message: `WorkUnit ${unit} depends on ${dependency}, which is not part of this Campaign projection.`,
          unitId: unit,
        });
        continue;
      }
      if (dependencyTask.sequence >= task.sequence) {
        issues.push({
          kind: "SEQUENCE_ORDER",
          message: `WorkUnit ${unit} (sequence ${task.sequence}) depends on ${dependency} (sequence ${dependencyTask.sequence}); the recorded sequence does not follow the dependency.`,
          unitId: unit,
        });
      }
      existing.push(dependency);
    }
    dependencies.set(unit, existing);
  }

  const hasCycle = detectCycle(units, dependencies);
  if (hasCycle) {
    issues.push({
      kind: "CYCLE",
      message:
        "The recorded dependency graph contains a cycle. The visual fallback places units by frozen sequence; no graph data was repaired.",
    });
  }

  const depth = new Map<string, number>();
  if (!hasCycle) {
    const remaining = new Set(units);
    while (remaining.size > 0) {
      let progressed = false;
      for (const unit of [...remaining]) {
        const deps = dependencies.get(unit) ?? [];
        if (deps.every((dependency) => depth.has(dependency))) {
          depth.set(
            unit,
            deps.length === 0
              ? 0
              : Math.max(
                  ...deps.map((dependency) => (depth.get(dependency) ?? 0) + 1),
                ),
          );
          remaining.delete(unit);
          progressed = true;
        }
      }
      if (!progressed) {
        issues.push({
          kind: "CYCLE",
          message:
            "Dependency depth could not be resolved; the fallback layout uses the frozen sequence order.",
        });
        break;
      }
    }
  }

  const layerFor = (unit: string, index: number) =>
    hasCycle ? index : (depth.get(unit) ?? index);
  const layers = new Map<number, string[]>();
  units.forEach((unit, index) => {
    const layer = layerFor(unit, index);
    const members = layers.get(layer) ?? [];
    members.push(unit);
    layers.set(layer, members);
  });

  const nodes: WorkGraphNode[] = [];
  for (const [layer, members] of [...layers.entries()].sort(
    (left, right) => left[0] - right[0],
  )) {
    members.sort((left, right) => {
      const leftTask = byUnit.get(left);
      const rightTask = byUnit.get(right);
      if (!leftTask || !rightTask) return left.localeCompare(right);
      return (
        leftTask.sequence - rightTask.sequence ||
        leftTask.unit_id.localeCompare(rightTask.unit_id)
      );
    });
    members.forEach((unit, row) => {
      const task = byUnit.get(unit);
      if (!task) return;
      nodes.push({
        task,
        layer,
        row,
        x: WORKGRAPH_PADDING + layer * (WORKGRAPH_NODE_WIDTH + WORKGRAPH_GAP_X),
        y: WORKGRAPH_PADDING + row * (WORKGRAPH_NODE_HEIGHT + WORKGRAPH_GAP_Y),
        state: presentationState(task, byUnit, issues),
      });
    });
  }

  const nodeByUnit = new Map(nodes.map((node) => [node.task.unit_id, node]));
  for (const node of nodes) {
    for (const dependency of node.task.depends_on) {
      const source = nodeByUnit.get(dependency);
      if (!source || source.task.unit_id === node.task.unit_id) continue;
      edges.push({
        from: source.task.unit_id,
        to: node.task.unit_id,
        targetState: node.state,
      });
    }
  }

  const nodeByTaskId = new Map(nodes.map((node) => [node.task.task_id, node]));
  const tableRows: WorkGraphTableRow[] = ordered.map((task) => {
    const node = nodeByTaskId.get(task.task_id);
    return {
      task,
      state: node?.state ?? "INCONSISTENT",
      graphNode: node !== undefined,
    };
  });

  const maxLayer = nodes.reduce((max, node) => Math.max(max, node.layer), 0);
  const maxRow = nodes.reduce((max, node) => Math.max(max, node.row), 0);
  return {
    nodes,
    tableRows,
    edges,
    issues,
    width:
      nodes.length === 0
        ? 0
        : WORKGRAPH_PADDING * 2 +
          (maxLayer + 1) * WORKGRAPH_NODE_WIDTH +
          maxLayer * WORKGRAPH_GAP_X,
    height:
      nodes.length === 0
        ? 0
        : WORKGRAPH_PADDING * 2 +
          (maxRow + 1) * WORKGRAPH_NODE_HEIGHT +
          maxRow * WORKGRAPH_GAP_Y,
    layerCount: maxLayer + 1,
    hasCycle,
  };
}

export function presentationLabel(state: WorkGraphPresentationState): string {
  if (state === "READY") return "READY (derived)";
  if (state === "WAITING") return "WAITING (derived)";
  if (state === "BLOCKED") return "BLOCKED (derived)";
  if (state === "INCONSISTENT") return "INCONSISTENT";
  return state;
}

export function presentationExplanation(
  state: WorkGraphPresentationState,
): string {
  if (state === "READY")
    return "Recorded PENDING with every dependency recorded SUCCEEDED (or no dependencies). This is a dependency fact, not scheduling progress.";
  if (state === "WAITING")
    return "Recorded PENDING while at least one dependency is not SUCCEEDED. This is a dependency fact, not an execution forecast.";
  if (state === "BLOCKED")
    return "Recorded PENDING while a dependency is FAILED or CANCELLED.";
  if (state === "INCONSISTENT")
    return "Recorded PENDING with a dependency that is not present in this Campaign projection. The UI does not repair the graph.";
  return "Exact recorded WorkUnit status.";
}
