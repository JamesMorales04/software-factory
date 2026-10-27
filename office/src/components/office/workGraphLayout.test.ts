import { describe, expect, it } from "vitest";
import type { LeadTask } from "../../coreApi";
import { buildWorkGraphLayout } from "./workGraphLayout";

function task(
  unitId: string,
  sequence: number,
  status = "PENDING",
  dependsOn: string[] = [],
  extra: Partial<LeadTask> = {},
): LeadTask {
  return {
    task_id: `task-${unitId}`,
    campaign_id: "campaign-1",
    request_id: "lead-1",
    unit_id: unitId,
    sequence,
    status,
    base_commit: "a".repeat(40),
    depends_on: dependsOn,
    run_id: null,
    attempt_id: null,
    candidate_id: null,
    candidate_commit: null,
    candidate_tree: null,
    updated_at: "2026-10-27T10:00:00Z",
    ...extra,
  };
}

function requireNode(
  layout: ReturnType<typeof buildWorkGraphLayout>,
  unitId: string,
) {
  const node = layout.nodes.find((item) => item.task.unit_id === unitId);
  if (!node) throw new Error(`missing node ${unitId}`);
  return node;
}

function rectsOverlap(
  left: { x: number; y: number },
  right: { x: number; y: number },
): boolean {
  return left.x === right.x && left.y === right.y;
}

describe("buildWorkGraphLayout", () => {
  it("handles an empty graph without inventing nodes", () => {
    const layout = buildWorkGraphLayout([]);
    expect(layout.nodes).toHaveLength(0);
    expect(layout.edges).toHaveLength(0);
    expect(layout.issues).toHaveLength(0);
    expect(layout.width).toBe(0);
    expect(layout.height).toBe(0);
  });

  it("lays out a single WorkUnit", () => {
    const layout = buildWorkGraphLayout([task("only", 0)]);
    expect(layout.nodes).toHaveLength(1);
    expect(layout.nodes[0].layer).toBe(0);
    expect(layout.nodes[0].state).toBe("READY");
    expect(layout.edges).toHaveLength(0);
  });

  it("lays out a linear dependency chain left to right", () => {
    const layout = buildWorkGraphLayout([
      task("a", 0, "SUCCEEDED"),
      task("b", 1, "SUCCEEDED", ["a"]),
      task("c", 2, "PENDING", ["b"]),
    ]);
    expect(layout.nodes.map((node) => node.layer)).toEqual([0, 1, 2]);
    expect(layout.nodes[2].state).toBe("READY");
    expect(layout.edges).toHaveLength(2);
  });

  it("places parallel siblings in one layer and a diamond fan-in after them", () => {
    const layout = buildWorkGraphLayout([
      task("root", 0, "SUCCEEDED"),
      task("left", 1, "SUCCEEDED", ["root"]),
      task("right", 2, "SUCCEEDED", ["root"]),
      task("join", 3, "PENDING", ["left", "right"]),
    ]);
    const byUnit = new Map(
      layout.nodes.map((node) => [node.task.unit_id, node]),
    );
    expect(requireNode(layout, "left").layer).toBe(1);
    expect(requireNode(layout, "right").layer).toBe(1);
    expect(requireNode(layout, "left").row).not.toBe(
      requireNode(layout, "right").row,
    );
    expect(requireNode(layout, "join").layer).toBe(2);
    expect(requireNode(layout, "join").state).toBe("READY");
    expect(layout.edges).toHaveLength(4);
  });

  it("keeps eight WorkUnits non-overlapping and stable", () => {
    const tasks = [
      task("a", 0, "SUCCEEDED"),
      task("b", 1, "SUCCEEDED", ["a"]),
      task("c", 2, "SUCCEEDED", ["a"]),
      task("d", 3, "SUCCEEDED", ["b"]),
      task("e", 4, "SUCCEEDED", ["c"]),
      task("f", 5, "SUCCEEDED", ["d", "e"]),
      task("g", 6, "PENDING", ["f"]),
      task("h", 7, "PENDING", ["f"]),
    ];
    const first = buildWorkGraphLayout(tasks);
    const second = buildWorkGraphLayout([...tasks].reverse());
    expect(first.nodes).toHaveLength(8);
    expect(first.nodes.map((node) => node.task.unit_id)).toEqual(
      second.nodes.map((node) => node.task.unit_id),
    );
    for (let left = 0; left < first.nodes.length; left += 1) {
      for (let right = left + 1; right < first.nodes.length; right += 1) {
        expect(rectsOverlap(first.nodes[left], first.nodes[right])).toBe(false);
        expect(
          Math.abs(first.nodes[left].x - first.nodes[right].x) >= 224 ||
            Math.abs(first.nodes[left].y - first.nodes[right].y) >= 96,
        ).toBe(true);
      }
    }
  });

  it("derives blocked and waiting dependency facts without changing recorded status", () => {
    const blocked = buildWorkGraphLayout([
      task("a", 0, "FAILED"),
      task("b", 1, "PENDING", ["a"]),
    ]);
    expect(blocked.nodes[1].state).toBe("BLOCKED");
    expect(blocked.nodes[1].task.status).toBe("PENDING");
    const waiting = buildWorkGraphLayout([
      task("a", 0, "RUNNING"),
      task("b", 1, "PENDING", ["a"]),
    ]);
    expect(waiting.nodes[1].state).toBe("WAITING");
  });

  it("surfaces missing dependencies, cycles, duplicates and unknown statuses", () => {
    const missing = buildWorkGraphLayout([task("a", 0, "PENDING", ["ghost"])]);
    expect(missing.nodes[0].state).toBe("INCONSISTENT");
    expect(
      missing.issues.some((issue) => issue.kind === "MISSING_DEPENDENCY"),
    ).toBe(true);

    const cycle = buildWorkGraphLayout([
      task("a", 0, "PENDING", ["b"]),
      task("b", 1, "PENDING", ["a"]),
    ]);
    expect(cycle.hasCycle).toBe(true);
    expect(cycle.issues.some((issue) => issue.kind === "CYCLE")).toBe(true);
    expect(cycle.nodes).toHaveLength(2);

    const duplicate = buildWorkGraphLayout([
      task("a", 0, "PENDING"),
      task("a", 1, "RUNNING"),
    ]);
    expect(
      duplicate.issues.some((issue) => issue.kind === "DUPLICATE_UNIT"),
    ).toBe(true);
    expect(duplicate.nodes).toHaveLength(1);

    const unknown = buildWorkGraphLayout([task("a", 0, "MYSTERY")]);
    expect(unknown.nodes[0].state).toBe("UNKNOWN");
    expect(
      unknown.issues.some((issue) => issue.kind === "UNKNOWN_STATUS"),
    ).toBe(true);

    const sequence = buildWorkGraphLayout([
      task("a", 2, "SUCCEEDED"),
      task("b", 1, "PENDING", ["a"]),
    ]);
    expect(
      sequence.issues.some((issue) => issue.kind === "SEQUENCE_ORDER"),
    ).toBe(true);
  });
});
