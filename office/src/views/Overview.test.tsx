import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Action, DecisionItem, OfficeOverview } from "../coreApi";
import { ActionDetail } from "./ActionDetail";
import { DecisionInbox } from "./DecisionInbox";
import { Overview } from "./Overview";

const mocks = vi.hoisted(() => ({
  getOverview: vi.fn(),
  getDecisions: vi.fn(),
  resolveModelCall: vi.fn(),
  listActions: vi.fn(),
  listProjects: vi.fn(),
  listCampaigns: vi.fn(),
  listLeadRequests: vi.fn(),
  listDelegations: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
}));

const decision: DecisionItem = {
  id: "model-call:reservation-1",
  kind: "model.usage-unresolved",
  severity: "critical",
  actionable: true,
  title: "Unresolved model usage",
  detail: "The model call is AMBIGUOUS.",
  target: "/runtime",
  subject_id: "reservation-1",
  updated_at: "2026-10-27T10:00:00Z",
  action: {
    kind: "resolve_model_call",
    model_call_id: "reservation-1",
    label: "Resolve usage",
  },
};

const action: Action = {
  id: "action-1",
  project_id: "project-1",
  objective: "Inspect the frozen Action.",
  provider: "opencode",
  model: "deepseek-v4.1-flash",
  risk: "UNASSESSED",
  status: "HUMAN_REQUIRED",
  objective_sha256: "objective-sha",
  created_at: "2026-10-27T10:00:00Z",
  spec_sha256: "spec-sha",
  spec: {
    schema_version: 1,
    project_id: "project-1",
    repository: "/tmp/project",
    base_commit: "0123456789abcdef",
    objective: "Inspect the frozen Action.",
    objective_sha256: "objective-sha",
    provider: "opencode",
    model: "deepseek-v4.1-flash",
    allowed_paths: ["src"],
    checks: [
      {
        id: "build",
        kind: "build",
        description: "build",
        command: ["true"],
        expectExit: 0,
        timeoutSecs: 30,
      },
    ],
  },
};

const overview: OfficeOverview = {
  as_of: "2026-10-27T10:00:00Z",
  source: "Core durable snapshots",
  counts: {
    projects: 1,
    actions: 2,
    campaigns: 3,
    runs: 1,
    attempts: 1,
    candidates: 1,
    workers: 1,
    lead_requests: 0,
    authorized_campaigns: 0,
    campaigns_awaiting_authorization: 1,
    active_agents: 0,
    unresolved_sessions: 0,
    unresolved_model_calls: 1,
    pending_decisions: 1,
    blocked_work: 1,
  },
  actions_by_status: { AUTHORIZED: 2 },
  campaigns_by_status: { AUTHORIZED: 3 },
  runs_by_status: { ACCEPTED: 1 },
  gate_outcomes: { ACCEPTED: 1 },
  budget: {
    measured_session_cost_micros: 1914,
    measured_session_tokens: 100,
    measured_model_cost_micros: 0,
    measured_model_tokens: 0,
    unmeasured_sessions: 0,
    unmeasured_model_calls: 1,
    model_calls_sampled: 1,
    note: "Measured records only.",
  },
  runtime: {
    core_healthy: true,
    workers: 1,
    active_agents: 0,
    unresolved_sessions: 0,
    unresolved_model_calls: 1,
    observability: {
      enabled: false,
      cursor: 0,
      exported: 0,
      pending: 0,
      dropped: 0,
      failed_attempts: 0,
      last_error: null,
    },
  },
  recent_accepted: [
    {
      candidate_id: "candidate-1",
      campaign_id: "campaign-1",
      commit_sha: "0123456789abcdef",
      decided_at: "2026-10-27T09:00:00Z",
      outcome: "ACCEPTED",
    },
  ],
  recent_decisions: [decision],
  recent_events: [],
};

function renderView(node: React.ReactNode) {
  return render(<MemoryRouter>{node}</MemoryRouter>);
}

describe("operational workspace", () => {
  beforeEach(() => {
    mocks.getOverview.mockReset().mockResolvedValue(overview);
    mocks.getDecisions.mockReset().mockResolvedValue({
      items: [decision],
      as_of: "2026-10-27T10:00:00Z",
    });
    mocks.resolveModelCall.mockReset().mockResolvedValue(undefined);
    mocks.listActions.mockReset().mockResolvedValue([action]);
    mocks.listProjects.mockReset().mockResolvedValue([]);
    mocks.listCampaigns.mockReset().mockResolvedValue([]);
    mocks.listLeadRequests.mockReset().mockResolvedValue([]);
    mocks.listDelegations.mockReset().mockResolvedValue([]);
  });

  it("renders truthful Overview counts and accepted candidates", async () => {
    renderView(<Overview />);
    await screen.findByRole("heading", { name: "Overview" });
    expect(screen.getByText("Unresolved model usage")).toBeDefined();
    expect(screen.getByText("ACCEPTED: 1")).toBeDefined();
    expect(screen.getByText("Measured spend")).toBeDefined();
    expect(screen.getByText("candidat")).toBeDefined();
  });

  it("renders Action detail from the existing Core projections", async () => {
    render(
      <MemoryRouter initialEntries={["/actions/action-1"]}>
        <Routes>
          <Route path="/actions/:actionId" element={<ActionDetail />} />
        </Routes>
      </MemoryRouter>,
    );
    await screen.findByRole("heading", { name: "Action action-1" });
    expect(screen.getByText("Frozen checks (1)")).toBeDefined();
    expect(screen.getByText("true")).toBeDefined();
  });

  it("resolves an ambiguous model call through the Core operation", async () => {
    renderView(<DecisionInbox />);
    await screen.findByRole("heading", { name: "Decision Inbox" });
    const reason = screen.getByLabelText("Resolution reason");
    const resolve = screen.getByRole("button", { name: "Resolve usage" });
    expect(resolve.hasAttribute("disabled")).toBe(true);
    await userEvent.type(reason, "Provider aborted before usage was reported.");
    await userEvent.click(resolve);
    await waitFor(() =>
      expect(mocks.resolveModelCall).toHaveBeenCalledWith(
        "reservation-1",
        "office-operator",
        "Provider aborted before usage was reported.",
      ),
    );
  });
});
