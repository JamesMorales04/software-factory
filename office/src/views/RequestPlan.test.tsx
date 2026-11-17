import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type {
  AgenticSpecification,
  LeadPlanProposalView,
  SpecCheck,
} from "../coreApi";
import { I18nProvider } from "../i18n";
import { RequestPlan } from "./RequestPlan";

const check: SpecCheck = {
  id: "test",
  kind: "test",
  description: "Run the test suite",
  command: ["cargo", "test"],
  expectExit: 0,
  timeoutSecs: 120,
};

function proposal(workUnits: LeadPlanProposalView["workUnits"]) {
  return {
    projectId: "project-1",
    actionId: "action-1",
    baseRevision: "base-revision",
    projectConfigHash: "config-hash",
    objective: "Add a CSV export endpoint",
    class: "feature",
    assumptions: [],
    risks: [],
    acceptanceCriteria: [check],
    negativeCases: [],
    qualityProfile: "generic",
    riskClass: "R0",
    budget: { maxAttempts: 2, timeoutSecs: 300 },
    route: {
      provider: "opencode",
      model: "deepseek-v4.1-flash",
      mode: "build",
    },
    maxRepairCycles: 1,
    maxWallClockSecs: 600,
    maxParallel: 1,
    maxSessions: 2,
    workUnits,
  } satisfies LeadPlanProposalView;
}

const workGraphUnit: AgenticSpecification = {
  id: "graph-unit",
  description: "Compile the export projection",
  kind: "implementation",
  allowedPaths: ["native/src/export.rs"],
  capabilities: [],
  dependsOn: [],
  status: "PLANNED",
};

function renderPlan(
  leadWorkUnits: LeadPlanProposalView["workUnits"],
  specifications: AgenticSpecification[],
  participantState: string,
) {
  return render(
    <I18nProvider initialLocale="en">
      <RequestPlan
        proposal={proposal(leadWorkUnits)}
        plan={null}
        specifications={specifications}
        participantState={participantState}
      />
    </I18nProvider>,
  );
}

describe("RequestPlan", () => {
  it("shows the typed Planner-pending state instead of a synthetic unit", () => {
    renderPlan([], [], "WORK_GRAPH_NOT_PLANNED");

    expect(screen.getByText("Planner pending.")).toBeDefined();
    expect(screen.getAllByText("WORK_GRAPH_NOT_PLANNED").length).toBeGreaterThan(0);
    expect(screen.queryByText("unit-1")).toBeNull();
    expect(screen.queryByText("No work units recorded.")).toBeNull();
  });

  it("renders validated WorkGraph units when the Lead authors none", () => {
    renderPlan([], [workGraphUnit], "PLANNED");

    expect(screen.getByText("WorkGraph units")).toBeDefined();
    expect(screen.getByText("graph-unit")).toBeDefined();
    expect(screen.getByText("Compile the export projection")).toBeDefined();
    expect(screen.getByText("PLANNED")).toBeDefined();
    expect(screen.queryByText("Planner pending.")).toBeNull();
  });
});
