import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CoreApiError,
  type ExecutionProfile,
  type IntakeRequestProjection,
  type IntakeRequestSummary,
  type OfficeOverview,
  type Project,
} from "../coreApi";
import { Home } from "./Home";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  listExecutionProfiles: vi.fn(),
  listIntakeRequests: vi.fn(),
  getOverview: vi.fn(),
  createIntakeRequest: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    readonly status: number;
    constructor(status: number, message: string) {
      super(message);
      this.name = "CoreApiError";
      this.status = status;
    }
  },
  newIdempotencyKey: () => "idem-test",
}));

const project: Project = {
  id: "project-1",
  repository: "https://github.com/acme/factory.git",
  base_commit: "0123456789abcdef0123456789abcdef01234567",
  created_at: "2026-10-27T10:00:00Z",
};

const profile: ExecutionProfile = {
  id: "profile-1",
  name: "Balanced",
  description: "Default supported routes",
  category: "BALANCED",
  revision: 3,
  revisionId: "revision-1",
  contentHash: "profile-hash",
  globalDefault: true,
  projectDefaults: [],
  roles: {},
  budgets: {
    maxAttempts: 3,
    maxSessions: 2,
    maxTurns: 4,
    maxInputTokens: 40000,
    maxOutputTokens: 12000,
    maxCostMicros: 500000,
    deadlineSecs: 600,
  },
  autoStart: { enabled: false, riskClasses: [] },
  executable: true,
  warnings: [],
  createdAt: "2026-10-27T10:00:00Z",
  updatedAt: "2026-10-27T10:00:00Z",
};

const summary: IntakeRequestSummary = {
  id: "request-1",
  projectId: "project-1",
  objective: "Add a CSV export endpoint",
  mode: "REVIEW_PLAN",
  state: "AWAITING_APPROVAL",
  stateDetail: "Frozen plan ready for explicit start",
  profile: {
    id: "profile-1",
    revisionId: "revision-1",
    revision: 3,
    name: "Balanced",
    contentHash: "profile-hash",
  },
  actionId: "action-1",
  delegationId: "delegation-1",
  leadRequestId: "lead-1",
  campaignId: null,
  authorization: { required: true, available: true, reason: null },
  createdAt: "2026-10-27T10:00:00Z",
  updatedAt: "2026-10-27T10:05:00Z",
};

const projection: IntakeRequestProjection = {
  ...summary,
  assessment: null,
  proposal: null,
  plan: null,
  campaign: null,
  messages: [],
};

const overview: OfficeOverview = {
  as_of: "2026-10-27T10:00:00Z",
  source: "Core durable snapshots",
  counts: {
    projects: 1,
    actions: 1,
    campaigns: 1,
    runs: 0,
    attempts: 0,
    candidates: 0,
    workers: 1,
    lead_requests: 1,
    authorized_campaigns: 0,
    campaigns_awaiting_authorization: 1,
    active_agents: 2,
    unresolved_sessions: 0,
    unresolved_model_calls: 1,
    pending_decisions: 1,
    blocked_signals: 1,
  },
  actions_by_status: {},
  campaigns_by_status: {},
  runs_by_status: {},
  gate_outcomes: {},
  budget: {
    measured_session_cost_micros: 0,
    measured_session_tokens: 0,
    measured_model_cost_micros: 0,
    measured_model_tokens: 0,
    unmeasured_sessions: 0,
    unmeasured_model_calls: 1,
    unmeasured_bound_cost_micros: 500000,
    model_calls_sampled: 0,
    note: "Measured records only.",
  },
  runtime: {
    core_healthy: true,
    workers: 1,
    active_agents: 2,
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
  recent_accepted: [],
  recent_decisions: [
    {
      id: "model-call:1",
      kind: "model.usage-unresolved",
      severity: "critical",
      actionable: true,
      title: "Unresolved model usage",
      detail: "The model call is AMBIGUOUS.",
      target: "/runtime",
      subject_id: "1",
      updated_at: "2026-10-27T10:00:00Z",
    },
  ],
  recent_events: [
    {
      sequence: 7,
      subject_id: "action-1",
      kind: "ActionCreated",
      detail: "Action recorded.",
      recorded_at: "2026-10-27T10:00:00Z",
    },
  ],
};

function renderHome() {
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route path="/" element={<Home />} />
        <Route
          path="/requests/:requestId"
          element={<div>Request workspace probe</div>}
        />
      </Routes>
    </MemoryRouter>,
  );
}

describe("Home", () => {
  beforeEach(() => {
    mocks.listProjects.mockReset().mockResolvedValue([project]);
    mocks.listExecutionProfiles.mockReset().mockResolvedValue([profile]);
    mocks.listIntakeRequests.mockReset().mockResolvedValue([summary]);
    mocks.getOverview.mockReset().mockResolvedValue(overview);
    mocks.createIntakeRequest.mockReset().mockResolvedValue(projection);
  });

  it("renders the operator-first request surface with Core-backed status", async () => {
    renderHome();
    await screen.findByRole("heading", { name: "Home" });
    expect(screen.getByLabelText("Project")).toBeDefined();
    expect(screen.getByLabelText("What should the Factory do?")).toBeDefined();
    expect(screen.getByLabelText("Execution profile")).toBeDefined();
    expect(
      (screen.getByRole("radio", { name: /Review plan/ }) as HTMLInputElement)
        .checked,
    ).toBe(true);
    expect(
      screen.getAllByText(/Core assesses the request, freezes a plan/).length,
    ).toBeGreaterThan(0);
    expect(await screen.findByText("Add a CSV export endpoint")).toBeDefined();
    expect(screen.getByText("Awaiting approval")).toBeDefined();
    expect(screen.getByText("Unresolved model usage")).toBeDefined();
    expect(screen.getByText("ActionCreated")).toBeDefined();
    expect(screen.getByText("Core status")).toBeDefined();
  });

  it("validates Project and objective before creating a request", async () => {
    renderHome();
    await screen.findByLabelText("Project");
    await userEvent.selectOptions(screen.getByLabelText("Project"), "");
    await userEvent.type(
      screen.getByLabelText("What should the Factory do?"),
      "Add an export endpoint.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Submit request" }),
    );
    expect(await screen.findByText("Select a Project.")).toBeDefined();
    expect(document.activeElement?.id).toBe("home-project");
    expect(mocks.createIntakeRequest).not.toHaveBeenCalled();

    await userEvent.selectOptions(
      screen.getByLabelText("Project"),
      "project-1",
    );
    await userEvent.clear(screen.getByLabelText("What should the Factory do?"));
    await userEvent.click(
      screen.getByRole("button", { name: "Submit request" }),
    );
    expect(
      await screen.findByText("Describe the engineering outcome you want."),
    ).toBeDefined();
    expect(document.activeElement?.id).toBe("home-objective");
    expect(mocks.createIntakeRequest).not.toHaveBeenCalled();
  });

  it("submits the selected Project, profile and mode to intake", async () => {
    renderHome();
    await screen.findByLabelText("Execution profile");
    await userEvent.selectOptions(
      screen.getByLabelText("Execution profile"),
      "profile-1",
    );
    await userEvent.click(screen.getByRole("radio", { name: /Autonomous/ }));
    expect(screen.getByText(/does not enable auto-start/)).toBeDefined();
    await userEvent.type(
      screen.getByLabelText("What should the Factory do?"),
      "Add a CSV export endpoint.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Submit request" }),
    );
    expect(await screen.findByText("Request workspace probe")).toBeDefined();
    expect(mocks.createIntakeRequest).toHaveBeenCalledWith({
      project_id: "project-1",
      objective: "Add a CSV export endpoint.",
      execution_profile_id: "profile-1",
      mode: "AUTONOMOUS",
      idempotency_key: "idem-test",
    });
  });

  it("disables duplicate submission while Core creates the request", async () => {
    let resolveCreate: ((value: IntakeRequestProjection) => void) | undefined;
    mocks.createIntakeRequest.mockReturnValue(
      new Promise<IntakeRequestProjection>((resolve) => {
        resolveCreate = resolve;
      }),
    );
    renderHome();
    await screen.findByLabelText("What should the Factory do?");
    await userEvent.type(
      screen.getByLabelText("What should the Factory do?"),
      "Add a CSV export endpoint.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Submit request" }),
    );
    const submitting = await screen.findByRole("button", {
      name: "Submitting request…",
    });
    expect((submitting as HTMLButtonElement).disabled).toBe(true);
    await act(async () => {
      resolveCreate?.(projection);
    });
    expect(await screen.findByText("Request workspace probe")).toBeDefined();
  });

  it("reports a missing unified intake endpoint without fabricating success", async () => {
    mocks.createIntakeRequest.mockRejectedValue(
      new CoreApiError(404, "not found"),
    );
    renderHome();
    await screen.findByLabelText("What should the Factory do?");
    await userEvent.type(
      screen.getByLabelText("What should the Factory do?"),
      "Add a CSV export endpoint.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Submit request" }),
    );
    expect(
      await screen.findByText(/Unified request intake is not available/),
    ).toBeDefined();
    expect(screen.queryByText("Request workspace probe")).toBeNull();
  });

  it("falls back to the Core-resolved default when profiles are unavailable", async () => {
    mocks.listExecutionProfiles.mockRejectedValue(
      new CoreApiError(404, "not found"),
    );
    renderHome();
    expect(
      await screen.findByText(
        /Execution profiles are not available from this Core build/,
      ),
    ).toBeDefined();
    expect(
      screen.getByRole("option", {
        name: "Core default (profile list unavailable)",
      }),
    ).toBeDefined();
    await userEvent.type(
      screen.getByLabelText("What should the Factory do?"),
      "Add a CSV export endpoint.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Submit request" }),
    );
    await waitFor(() => expect(mocks.createIntakeRequest).toHaveBeenCalled());
    expect(mocks.createIntakeRequest).toHaveBeenCalledWith({
      project_id: "project-1",
      objective: "Add a CSV export endpoint.",
      mode: "REVIEW_PLAN",
      idempotency_key: "idem-test",
    });
  });

  it("shows an explicit empty Project state instead of a fake composer", async () => {
    mocks.listProjects.mockResolvedValue([]);
    renderHome();
    expect(await screen.findByText("Register a Project first")).toBeDefined();
    expect(screen.queryByLabelText("What should the Factory do?")).toBeNull();
    expect(
      screen.getByRole("link", { name: "Open Projects" }).getAttribute("href"),
    ).toBe("/projects");
  });

  it("keeps the composer usable when Core status is unavailable", async () => {
    mocks.getOverview.mockRejectedValue(new Error("connection refused"));
    renderHome();
    expect(
      await screen.findByText(/Core status is unavailable: connection refused/),
    ).toBeDefined();
    expect(screen.getByLabelText("What should the Factory do?")).toBeDefined();
  });

  it("does not present an empty request list when the projection is unavailable", async () => {
    mocks.listIntakeRequests.mockRejectedValue(
      new CoreApiError(404, "missing"),
    );
    renderHome();
    expect(
      await screen.findByText(/Unified request history is not available/),
    ).toBeDefined();
    expect(screen.queryByText("No requests yet")).toBeNull();
  });
});
