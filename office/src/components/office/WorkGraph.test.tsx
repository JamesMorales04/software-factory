import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";
import type { Gate, LeadTask, QualityReceipt } from "../../coreApi";
import { WorkGraphPanel } from "./WorkGraph";

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

const acceptedGate: Gate = {
  candidate_id: "candidate-1",
  outcome: "ACCEPTED",
  reasons: [],
  decided_at: "2026-10-27T10:09:00Z",
};

const receipts: QualityReceipt[] = [
  {
    receipt_id: "verifier-1",
    candidate_id: "candidate-1",
    role: "verifier",
    principal_id: "verifier-1",
    action_spec_sha256: "spec",
    commit_sha: "b".repeat(40),
    tree_sha: "c".repeat(40),
    result: "PASS",
    evidence: "EXECUTED",
    findings: [],
    mechanism: "DETERMINISTIC",
    model_provider: "",
    model_name: "",
    model_reservation_id: "",
    model_request_sha256: "",
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
    cost_micros: 0,
    usage_known: false,
    received_at: "2026-10-27T10:08:00Z",
  },
  {
    receipt_id: "reviewer-1",
    candidate_id: "candidate-1",
    role: "reviewer",
    principal_id: "reviewer-1",
    action_spec_sha256: "spec",
    commit_sha: "b".repeat(40),
    tree_sha: "c".repeat(40),
    result: "PASS",
    evidence: "EXECUTED",
    findings: [],
    mechanism: "DETERMINISTIC",
    model_provider: "",
    model_name: "",
    model_reservation_id: "",
    model_request_sha256: "",
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
    cost_micros: 0,
    usage_known: false,
    received_at: "2026-10-27T10:08:30Z",
  },
];

function Harness({
  tasks,
  gates = {},
  receipts = {},
  stale = false,
  selectedUnitMissing = false,
}: {
  tasks: LeadTask[];
  gates?: Record<string, Gate | null>;
  receipts?: Record<string, QualityReceipt[]>;
  stale?: boolean;
  selectedUnitMissing?: boolean;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  return (
    <MemoryRouter>
      <WorkGraphPanel
        tasks={tasks}
        campaignId="campaign-1"
        selectedUnitId={selected}
        onSelectUnit={setSelected}
        sessions={[]}
        gates={gates}
        receipts={receipts}
        stale={stale}
        selectedUnitMissing={selectedUnitMissing}
      />
    </MemoryRouter>
  );
}

describe("WorkGraphPanel", () => {
  it("preserves the accessible table for an empty graph", () => {
    render(<Harness tasks={[]} />);
    expect(screen.getByText("No WorkGraph units")).toBeDefined();
    expect(screen.queryByTestId("workgraph-table")).toBeNull();
  });

  it("renders one WorkUnit as a graph node and table row", () => {
    render(<Harness tasks={[task("only", 0, "PENDING")]} />);
    expect(screen.getByTestId("workgraph-node-only")).toBeDefined();
    const table = screen.getByTestId("workgraph-table");
    expect(within(table).getByText("only")).toBeDefined();
    expect(within(table).getByText("READY (derived)")).toBeDefined();
  });

  it("renders a linear chain with dependency edges and selects a unit", async () => {
    render(
      <Harness
        tasks={[
          task("a", 0, "SUCCEEDED", [], { candidate_id: "candidate-1" }),
          task("b", 1, "PENDING", ["a"]),
        ]}
        gates={{ "candidate-1": acceptedGate }}
      />,
    );
    const a = screen.getByTestId("workgraph-node-a");
    await userEvent.click(a);
    expect(screen.getByTestId("workgraph-inspector")).toBeDefined();
    expect(screen.getByRole("heading", { name: /WorkUnit a/ })).toBeDefined();
    expect(
      within(screen.getByTestId("workgraph-inspector")).getByText("ACCEPTED"),
    ).toBeDefined();
    expect(screen.getByText(/No verifier receipt is recorded/)).toBeDefined();
  });

  it("supports keyboard movement and selection", async () => {
    render(
      <Harness
        tasks={[task("a", 0, "SUCCEEDED"), task("b", 1, "PENDING", ["a"])]}
      />,
    );
    const a = screen.getByTestId("workgraph-node-a");
    a.focus();
    await userEvent.keyboard("{ArrowRight}");
    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByTestId("workgraph-node-b"),
      ),
    );
    await userEvent.keyboard("{Enter}");
    expect(screen.getByTestId("workgraph-inspector")).toBeDefined();
  });

  it("surfaces failed, blocked and unknown statuses without repairing data", async () => {
    render(
      <Harness
        tasks={[
          task("failed", 0, "FAILED"),
          task("blocked", 1, "PENDING", ["failed"]),
          task("unknown", 2, "MYSTERY"),
        ]}
      />,
    );
    expect(screen.getByText("Graph inconsistencies")).toBeDefined();
    expect(
      screen.getByText(/unrecognized recorded status MYSTERY/),
    ).toBeDefined();
    await userEvent.click(screen.getByTestId("workgraph-node-blocked"));
    expect(
      screen.getByText(/Recorded PENDING while a dependency is FAILED/),
    ).toBeDefined();
    expect(screen.getByTestId("workgraph-table")).toBeDefined();
  });

  it("shows a missing candidate and a candidate without a Gate truthfully", async () => {
    render(
      <Harness
        tasks={[
          task("missing-candidate", 0, "SUCCEEDED", [], {
            attempt_id: "attempt-1",
            run_id: "run-1",
          }),
          task("pending-gate", 1, "SUCCEEDED", [], {
            candidate_id: "candidate-2",
          }),
        ]}
      />,
    );
    await userEvent.click(
      screen.getByTestId("workgraph-node-missing-candidate"),
    );
    expect(
      screen.getByText(/recorded SUCCEEDED but no Candidate is bound/),
    ).toBeDefined();
    await userEvent.click(screen.getByTestId("workgraph-node-pending-gate"));
    expect(
      screen.getByText(/No Quality Gate decision is recorded/),
    ).toBeDefined();
    expect(screen.queryByText("ACCEPTED")).toBeNull();
  });

  it("shows accepted evidence only when the exact Gate supports it", async () => {
    render(
      <Harness
        tasks={[
          task("accepted", 0, "SUCCEEDED", [], {
            candidate_id: "candidate-1",
            candidate_commit: "b".repeat(40),
            candidate_tree: "c".repeat(40),
          }),
        ]}
        gates={{ "candidate-1": acceptedGate }}
        receipts={{ "candidate-1": receipts }}
      />,
    );
    await userEvent.click(screen.getByTestId("workgraph-node-accepted"));
    const inspector = screen.getByTestId("workgraph-inspector");
    expect(within(inspector).getByText("ACCEPTED")).toBeDefined();
    expect(within(inspector).getAllByText("PASS")).toHaveLength(2);
  });

  it("shows malformed graph and stale projection states but keeps the table", () => {
    render(
      <Harness
        tasks={[task("a", 0, "PENDING", ["ghost"])]}
        stale
        selectedUnitMissing
      />,
    );
    expect(screen.getByText(/Graph inconsistencies/)).toBeDefined();
    expect(
      screen.getByText(/depends on ghost, which is not part of this Campaign/),
    ).toBeDefined();
    expect(screen.getByText(/projection is stale/)).toBeDefined();
    expect(
      screen.getByText(/selected WorkUnit is not part of this Campaign/),
    ).toBeDefined();
    expect(screen.getByTestId("workgraph-table")).toBeDefined();
  });

  it("preserves duplicate authoritative tasks in the table while reporting them", () => {
    render(
      <Harness
        tasks={[
          task("duplicate", 0, "PENDING", [], { task_id: "task-first" }),
          task("duplicate", 1, "RUNNING", [], { task_id: "task-second" }),
        ]}
      />,
    );
    const table = screen.getByTestId("workgraph-table");
    expect(within(table).getAllByRole("row")).toHaveLength(3);
    expect(within(table).getByText("task-second")).toBeDefined();
    expect(
      within(table).getAllByText("INCONSISTENT", { exact: false }).length,
    ).toBeGreaterThan(0);
  });
});
