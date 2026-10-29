import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CoreApiError,
  type ExecutionProfile,
  type IntakeRequestProjection,
  type IntakeRequestSummary,
  type LeadAssessmentView,
  type OfficeOverview,
  type Project,
} from "../coreApi";
import { I18nProvider, type Locale } from "../i18n";
import { Home } from "./Home";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  listExecutionProfiles: vi.fn(),
  listIntakeRequests: vi.fn(),
  getIntakeRequest: vi.fn(),
  getOverview: vi.fn(),
  createIntakeRequest: vi.fn(),
  enableExecutionPolicy: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    readonly status: number;
    readonly code: string | null;
    readonly reason: string | null;
    readonly body: unknown;
    constructor(status: number, message: string, body?: unknown) {
      super(message);
      this.name = "CoreApiError";
      this.status = status;
      this.body = body ?? null;
      const record =
        typeof body === "object" && body !== null
          ? (body as Record<string, unknown>)
          : null;
      this.code =
        record && typeof record.code === "string" ? record.code : null;
      const authorization =
        record && typeof record.authorization === "object"
          ? (record.authorization as Record<string, unknown>)
          : null;
      this.reason =
        authorization && typeof authorization.reason === "string"
          ? authorization.reason
          : this.code;
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

const roleRoute = {
  provider: "opencode-go",
  model: "deepseek-v4.1-flash",
  reasoning: "medium" as const,
};

const operations = {
  modelRequestTimeoutSecs: 300,
  toolCallTimeoutSecs: 600,
  maxRequestBytes: 2097152,
  maxResponseBytes: 2097152,
  maxGenerationTokens: 8192,
  maxInFlightOperations: 4,
};

const progress = {
  diagnoseAfterStagnantCheckpoints: 2,
  escalateAfterUnsuccessfulReplans: 1,
  observationTimeoutSecs: 300,
};

const profile: ExecutionProfile = {
  id: "profile-1",
  name: "Balanced",
  description: "Default supported routes",
  category: "BALANCED",
  schemaVersion: 2,
  revision: 3,
  revisionId: "revision-1",
  contentHash: "profile-hash",
  globalDefault: true,
  projectDefaults: [],
  roles: {
    lead: roleRoute,
    implementer: roleRoute,
    semanticReviewer: roleRoute,
  },
  operations,
  progress,
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

const assessment: LeadAssessmentView = {
  schemaRevision: 1,
  projectId: "project-1",
  actionId: "action-1",
  requestId: "lead-1",
  evidenceRefs: ["src/api.rs"],
  intent: "Add a supported export endpoint",
  complexity: "moderate",
  effortEstimate: {
    status: "estimated",
    unit: "person-hours",
    minPersonHours: 2,
    maxPersonHours: 4,
    assumptions: ["Existing API remains stable"],
    unknownReason: "",
  },
  knowledgeGaps: [],
  changeImpact: ["Adds a new endpoint"],
  validationNeeds: ["Integration test"],
  assumptions: ["Tests run in CI"],
  uncertainty: [],
  suggestedNextAction: "Review the frozen plan.",
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

function renderHome(locale?: Locale) {
  const content = (
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route path="/" element={<Home />} />
        <Route
          path="/requests/:requestId"
          element={<div>Request workspace probe</div>}
        />
      </Routes>
    </MemoryRouter>
  );
  return render(
    locale ? (
      <I18nProvider initialLocale={locale}>{content}</I18nProvider>
    ) : (
      content
    ),
  );
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

describe("Home", () => {
  beforeEach(() => {
    mocks.listProjects.mockReset().mockResolvedValue([project]);
    mocks.listExecutionProfiles
      .mockReset()
      .mockResolvedValue({ profiles: [profile], supportedRoutes: [] });
    mocks.listIntakeRequests.mockReset().mockResolvedValue([summary]);
    mocks.getIntakeRequest.mockReset().mockResolvedValue(projection);
    mocks.getOverview.mockReset().mockResolvedValue(overview);
    mocks.createIntakeRequest.mockReset().mockResolvedValue(projection);
    mocks.enableExecutionPolicy.mockReset().mockResolvedValue({
      grant: {},
      delegation: {},
      usage: {},
      replayed: false,
    });
  });

  it("renders the operator-first conversation surface with Core-backed status", async () => {
    renderHome();
    await screen.findByRole("heading", { name: "Home" });
    expect(
      screen.getByRole("heading", {
        name: "What do you want to build today?",
      }),
    ).toBeDefined();
    expect(screen.getByText("Conversation with your Lead")).toBeDefined();
    expect(screen.getByLabelText("Project")).toBeDefined();
    expect(screen.getByLabelText("Your instruction to the Lead")).toBeDefined();
    expect(screen.getByLabelText("Execution profile")).toBeDefined();
    expect(screen.queryByRole("radio", { name: /Autonomous/ })).toBeNull();
    expect(screen.getByText(/Explicit approval required/)).toBeDefined();
    const thread = await screen.findByTestId("home-thread");
    expect(within(thread).getByText("Your Lead")).toBeDefined();
    expect(
      await within(thread).findByText("The plan is ready for your approval."),
    ).toBeDefined();
    expect(
      within(thread).getByRole("link", { name: "Continue conversation" }),
    ).toBeDefined();
    const recent = screen.getByTestId("home-recent");
    expect(within(recent).getByText("Add a CSV export endpoint")).toBeDefined();
    expect(within(recent).getByText("Awaiting approval")).toBeDefined();
    expect(screen.getByText("Unresolved model usage")).toBeDefined();
    expect(screen.getByText("Factory status")).toBeDefined();
  });

  it("ignores a stale thread response after Core selects a newer request", async () => {
    vi.useFakeTimers();
    const staleResponse = deferred<IntakeRequestProjection>();
    const newerSummary: IntakeRequestSummary = {
      ...summary,
      id: "request-2",
      objective: "Inspect a newer request",
      state: "ASSESSING",
      stateDetail: "Assessment is in progress",
    };
    const staleProjection: IntakeRequestProjection = {
      ...projection,
      assessment: { ...assessment, suggestedNextAction: "Stale assessment" },
    };
    const newerProjection: IntakeRequestProjection = {
      ...projection,
      ...newerSummary,
      assessment: { ...assessment, suggestedNextAction: "Current assessment" },
    };
    mocks.listIntakeRequests
      .mockReset()
      .mockResolvedValueOnce([summary])
      .mockResolvedValue([newerSummary]);
    mocks.getIntakeRequest.mockImplementation((requestId: string) =>
      requestId === summary.id
        ? staleResponse.promise
        : Promise.resolve(newerProjection),
    );

    const view = renderHome();
    try {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(mocks.getIntakeRequest).toHaveBeenCalledWith(summary.id);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(8000);
      });
      expect(mocks.getIntakeRequest).toHaveBeenCalledWith(newerSummary.id);

      await act(async () => {
        staleResponse.resolve(staleProjection);
        await Promise.resolve();
      });
      const thread = screen.getByTestId("home-thread");
      expect(
        within(thread).getByText(/Next: Current assessment/),
      ).toBeDefined();
      expect(within(thread).queryByText(/Next: Stale assessment/)).toBeNull();
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });

  it("waits for each thread poll to settle before scheduling another", async () => {
    vi.useFakeTimers();
    const firstResponse = deferred<IntakeRequestProjection>();
    mocks.getIntakeRequest
      .mockReset()
      .mockReturnValueOnce(firstResponse.promise)
      .mockResolvedValue(projection);

    const view = renderHome();
    try {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(mocks.getIntakeRequest).toHaveBeenCalledTimes(1);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(24_000);
      });
      expect(mocks.getIntakeRequest).toHaveBeenCalledTimes(1);

      await act(async () => {
        firstResponse.resolve(projection);
        await Promise.resolve();
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(8000);
      });
      expect(mocks.getIntakeRequest).toHaveBeenCalledTimes(2);
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });

  it("does not require productivity caps or expose budget tables on Home", async () => {
    renderHome();
    await screen.findByLabelText("Execution profile");
    expect(screen.queryByLabelText(/Max turns/)).toBeNull();
    expect(screen.queryByLabelText(/Max attempts/)).toBeNull();
    expect(screen.queryByLabelText(/Max sessions/)).toBeNull();
    expect(screen.queryByLabelText(/Campaign max input tokens/)).toBeNull();
    expect(screen.queryByLabelText(/Campaign max output tokens/)).toBeNull();
    expect(screen.queryByLabelText(/Campaign max cost/)).toBeNull();
    expect(screen.queryByLabelText(/Campaign deadline/)).toBeNull();
    expect(screen.queryByText("Campaign budgets")).toBeNull();
  });

  it("renders the operator request surface in Spanish", async () => {
    renderHome("es");
    await screen.findByRole("heading", { name: "Inicio" });
    expect(
      screen.getByRole("heading", {
        name: "¿Qué quieres construir hoy?",
      }),
    ).toBeDefined();
    expect(screen.getByLabelText("Proyecto")).toBeDefined();
    expect(screen.getByLabelText("Tu instrucción al Lead")).toBeDefined();
    expect(screen.getByLabelText("Perfil de ejecución")).toBeDefined();
    expect(
      screen.getByLabelText("Autorización de inferencia (USD)"),
    ).toBeDefined();
    expect(screen.getByText(/Aprobación explícita obligatoria/)).toBeDefined();
    const recent = await screen.findByTestId("home-recent");
    expect(within(recent).getByText("Pendiente de aprobación")).toBeDefined();
    expect(screen.getByText("Estado de la Factory")).toBeDefined();
    expect(
      screen.getByRole("button", { name: "Enviar al Lead" }),
    ).toBeDefined();
  });

  it("validates Project and objective before creating a request", async () => {
    renderHome();
    await screen.findByLabelText("Project");
    await userEvent.selectOptions(screen.getByLabelText("Project"), "");
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add an export endpoint.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    expect(await screen.findByText("Select a Project.")).toBeDefined();
    expect(document.activeElement?.id).toBe("home-project");
    expect(mocks.createIntakeRequest).not.toHaveBeenCalled();

    await userEvent.selectOptions(
      screen.getByLabelText("Project"),
      "project-1",
    );
    await userEvent.clear(
      screen.getByLabelText("Your instruction to the Lead"),
    );
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    expect(
      await screen.findByText("Describe the engineering outcome you want."),
    ).toBeDefined();
    expect(document.activeElement?.id).toBe("home-objective");
    expect(mocks.createIntakeRequest).not.toHaveBeenCalled();
  });

  it("requires a positive inference spend authorization and sends the edited amount", async () => {
    renderHome();
    await screen.findByLabelText("Project");
    await userEvent.selectOptions(
      screen.getByLabelText("Project"),
      "project-1",
    );
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add an export endpoint.",
    );
    const spend = screen.getByLabelText("Inference authorization (USD)");
    await userEvent.clear(spend);
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    expect(
      await screen.findByText(
        "Enter a positive USD amount for the inference envelope.",
      ),
    ).toBeDefined();
    expect(document.activeElement?.id).toBe("home-spend");
    expect(mocks.createIntakeRequest).not.toHaveBeenCalled();

    await userEvent.type(spend, "10.50");
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    await waitFor(() => expect(mocks.createIntakeRequest).toHaveBeenCalled());
    expect(mocks.createIntakeRequest).toHaveBeenCalledWith(
      expect.objectContaining({ authorized_micros: 10_500_000 }),
    );
  });

  it("requires a positive inference spend authorization", async () => {
    renderHome();
    await screen.findByLabelText("Project");
    await userEvent.selectOptions(
      screen.getByLabelText("Project"),
      "project-1",
    );
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add an export endpoint.",
    );
    await userEvent.clear(
      screen.getByLabelText("Inference authorization (USD)"),
    );
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    expect(await screen.findByText(/positive USD amount/i)).toBeDefined();
    expect(mocks.createIntakeRequest).not.toHaveBeenCalled();
  });

  it("rejects zero, negative and unsafe inference spend amounts", async () => {
    renderHome();
    await screen.findByLabelText("Project");
    await userEvent.selectOptions(
      screen.getByLabelText("Project"),
      "project-1",
    );
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add an export endpoint.",
    );
    const spend = screen.getByLabelText("Inference authorization (USD)");
    for (const invalid of ["0", "-1", "999999999999999"]) {
      await userEvent.clear(spend);
      await userEvent.type(spend, invalid);
      await userEvent.click(
        screen.getByRole("button", { name: "Send to Lead" }),
      );
      expect(await screen.findByText(/positive USD amount/i)).toBeDefined();
      expect(mocks.createIntakeRequest).not.toHaveBeenCalled();
    }
  });

  it("submits an edited spend authorization in integer micros", async () => {
    renderHome();
    await screen.findByLabelText("Project");
    await userEvent.selectOptions(
      screen.getByLabelText("Project"),
      "project-1",
    );
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add an export endpoint.",
    );
    const spend = screen.getByLabelText("Inference authorization (USD)");
    await userEvent.clear(spend);
    await userEvent.type(spend, "12.50");
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    await waitFor(() => expect(mocks.createIntakeRequest).toHaveBeenCalled());
    expect(mocks.createIntakeRequest).toHaveBeenCalledWith(
      expect.objectContaining({ authorized_micros: 12_500_000 }),
    );
  });

  it("rejects zero, negative and over-limit spend authorizations", async () => {
    renderHome();
    await screen.findByLabelText("Project");
    await userEvent.selectOptions(
      screen.getByLabelText("Project"),
      "project-1",
    );
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add an export endpoint.",
    );
    const spend = screen.getByLabelText("Inference authorization (USD)");
    for (const value of ["0", "-1", "99999999999999"]) {
      await userEvent.clear(spend);
      await userEvent.type(spend, value);
      await userEvent.click(
        screen.getByRole("button", { name: "Send to Lead" }),
      );
      expect(await screen.findByText(/positive USD amount/i)).toBeDefined();
      expect(mocks.createIntakeRequest).not.toHaveBeenCalled();
    }
  });

  it("submits the selected Project and profile with explicit review-plan intent", async () => {
    renderHome();
    await screen.findByLabelText("Execution profile");
    await userEvent.selectOptions(
      screen.getByLabelText("Execution profile"),
      "profile-1",
    );
    expect(screen.queryByRole("radio", { name: /Autonomous/ })).toBeNull();
    expect(screen.getAllByText("opencode-go/")).toHaveLength(3);
    expect(screen.getAllByText("deepseek-v4.1-flash")).toHaveLength(3);
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add a CSV export endpoint.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    expect(await screen.findByText("Request workspace probe")).toBeDefined();
    expect(mocks.createIntakeRequest).toHaveBeenCalledWith({
      project_id: "project-1",
      objective: "Add a CSV export endpoint.",
      execution_profile_id: "profile-1",
      mode: "REVIEW_PLAN",
      idempotency_key: "idem-test",
      authorized_micros: 5_000_000,
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
    await screen.findByLabelText("Your instruction to the Lead");
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add a CSV export endpoint.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    const submitting = await screen.findByRole("button", {
      name: "Sending…",
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
    await screen.findByLabelText("Your instruction to the Lead");
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add a CSV export endpoint.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
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
        name: "Default profile (list unavailable)",
      }),
    ).toBeDefined();
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add a CSV export endpoint.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    await waitFor(() => expect(mocks.createIntakeRequest).toHaveBeenCalled());
    expect(mocks.createIntakeRequest).toHaveBeenCalledWith({
      project_id: "project-1",
      objective: "Add a CSV export endpoint.",
      mode: "REVIEW_PLAN",
      idempotency_key: "idem-test",
      authorized_micros: 5_000_000,
    });
  });

  it("shows an explicit empty Project state instead of a fake composer", async () => {
    mocks.listProjects.mockResolvedValue([]);
    renderHome();
    expect(await screen.findByText("Register a Project first")).toBeDefined();
    expect(screen.queryByLabelText("Your instruction to the Lead")).toBeNull();
    expect(
      screen.getByRole("link", { name: "Open Projects" }).getAttribute("href"),
    ).toBe("/projects");
  });

  it("keeps the composer usable when Factory status is unavailable", async () => {
    mocks.getOverview.mockRejectedValue(new Error("connection refused"));
    renderHome();
    expect(
      await screen.findByText(
        /Factory status is unavailable: connection refused/,
      ),
    ).toBeDefined();
    expect(screen.getByLabelText("Your instruction to the Lead")).toBeDefined();
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

  it("frames Home as a conversation and lets a suggestion seed the composer", async () => {
    renderHome();
    const instruction = await screen.findByLabelText(
      "Your instruction to the Lead",
    );
    expect(screen.getByRole("group", { name: "Suggestions" })).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Improve performance" }),
    );
    expect(document.activeElement).toBe(instruction);
    expect((instruction as HTMLTextAreaElement).value).toBe(
      "Improve performance in ",
    );

    await userEvent.click(screen.getByRole("button", { name: "Add tests" }));
    expect((instruction as HTMLTextAreaElement).value).toBe(
      "Improve performance in \nAdd tests for ",
    );
    expect(mocks.createIntakeRequest).not.toHaveBeenCalled();
  });

  it("submits the instruction with Ctrl+Enter without a mouse", async () => {
    renderHome();
    const instruction = await screen.findByLabelText(
      "Your instruction to the Lead",
    );
    await userEvent.type(instruction, "Add a CSV export endpoint.");
    await userEvent.keyboard("{Control>}{Enter}{/Control}");
    await waitFor(() => expect(mocks.createIntakeRequest).toHaveBeenCalled());
    expect(mocks.createIntakeRequest).toHaveBeenCalledWith(
      expect.objectContaining({
        objective: "Add a CSV export endpoint.",
      }),
    );
  });

  it("shows the latest active Lead thread and continues it in the workspace", async () => {
    const activeSummary: IntakeRequestSummary = {
      ...summary,
      id: "request-active",
      objective: "Harden device session revocation",
      updatedAt: "2026-10-27T10:06:00Z",
    };
    const completedSummary: IntakeRequestSummary = {
      ...summary,
      id: "request-completed",
      objective: "Old completed work",
      state: "COMPLETED",
      updatedAt: "2026-10-27T09:00:00Z",
    };
    const activeProjection: IntakeRequestProjection = {
      ...projection,
      ...activeSummary,
      assessment,
      messages: [
        {
          id: "message-1",
          role: "operator",
          body: "Keep existing clients working.",
          createdAt: "2026-10-27T10:04:00Z",
        },
      ],
    };
    mocks.listIntakeRequests.mockResolvedValue([
      completedSummary,
      activeSummary,
    ]);
    mocks.getIntakeRequest.mockResolvedValue(activeProjection);

    renderHome();
    const thread = await screen.findByTestId("home-thread");
    expect(
      await within(thread).findByText("Harden device session revocation"),
    ).toBeDefined();
    expect(
      within(thread).getByText("Keep existing clients working."),
    ).toBeDefined();
    expect(
      within(thread).getByText("Next: Review the frozen plan."),
    ).toBeDefined();
    expect(
      within(thread)
        .getByRole("link", { name: "Continue conversation" })
        .getAttribute("href"),
    ).toBe("/requests/request-active");
    await waitFor(() =>
      expect(mocks.getIntakeRequest).toHaveBeenCalledWith("request-active"),
    );
  });

  it("does not invent a Lead thread when there is no request history", async () => {
    mocks.listIntakeRequests.mockResolvedValue([]);
    renderHome();
    expect(await screen.findByText("No requests yet")).toBeDefined();
    expect(screen.queryByTestId("home-thread")).toBeNull();
    expect(mocks.getIntakeRequest).not.toHaveBeenCalled();
  });

  it("keeps the latest thread continuation available when its preview fails", async () => {
    mocks.getIntakeRequest.mockRejectedValue(new Error("offline"));
    renderHome();
    const thread = await screen.findByTestId("home-thread");
    expect(
      await within(thread).findByText(
        "The conversation preview could not be loaded.",
      ),
    ).toBeDefined();
    expect(
      within(thread).getByRole("link", { name: "Continue conversation" }),
    ).toBeDefined();
  });

  it("offers an explicit governed-execution grant and retries the same request", async () => {
    mocks.createIntakeRequest
      .mockRejectedValueOnce(
        new CoreApiError(409, "execution policy required", {
          code: "EXECUTION_POLICY_REQUIRED",
          authorization: {
            required: true,
            available: false,
            reason: "EXECUTION_POLICY_REQUIRED",
          },
          scope: {
            project: {
              id: "project-1",
              configHash: "config-hash",
              repository: "https://github.com/acme/factory.git",
              baseCommit: "0123456789abcdef0123456789abcdef01234567",
              allowedPaths: ["src"],
              capabilities: ["repository-edit"],
              qualityProfile: "generic",
            },
            profile: {
              id: "profile-1",
              revisionId: "revision-1",
              revision: 3,
              name: "Balanced",
              contentHash: "profile-hash",
              schemaVersion: 2,
              executable: true,
              warnings: [],
              operations,
              progress,
              autoStart: { enabled: false, riskClasses: [] },
              implementer: roleRoute,
              grantTerms: {
                objectiveClasses: ["feature", "maintenance", "refactor"],
                riskClasses: ["R0"],
                autonomousStart: false,
                validityDays: 30,
                maxParallel: 1,
                maxTimeoutSecs: 600,
                progress,
                operations,
              },
            },
          },
        }),
      )
      .mockResolvedValueOnce(projection);
    renderHome();
    await screen.findByLabelText("Your instruction to the Lead");
    await userEvent.type(
      screen.getByLabelText("Your instruction to the Lead"),
      "Add a CSV export endpoint.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Send to Lead" }));
    expect(
      await screen.findByRole("heading", {
        name: "Enable governed execution",
      }),
    ).toBeDefined();
    expect(screen.getAllByText(/Balanced r3/).length).toBeGreaterThan(0);
    expect(screen.getByText(/repository-edit/)).toBeDefined();
    expect(screen.getByText(/feature, maintenance, refactor/)).toBeDefined();
    expect(screen.getByText(/Valid 30 days/)).toBeDefined();
    expect(
      screen.getByText(/non-autonomous; explicit approval required/),
    ).toBeDefined();
    const enable = screen.getByRole("button", {
      name: "Enable governed execution",
    });
    expect((enable as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: /I authorize this bounded non-autonomous R0 execution policy/,
      }),
    );
    expect((enable as HTMLButtonElement).disabled).toBe(false);
    await userEvent.click(enable);
    await waitFor(() =>
      expect(mocks.enableExecutionPolicy).toHaveBeenCalledWith("project-1", {
        execution_profile_id: "profile-1",
        expected_revision: 3,
        expected_revision_hash: "profile-hash",
        project_config_hash: "config-hash",
        acknowledge: true,
        idempotency_key: "idem-test",
      }),
    );
    expect(await screen.findByText("Request workspace probe")).toBeDefined();
    expect(mocks.createIntakeRequest).toHaveBeenCalledTimes(2);
    expect(mocks.createIntakeRequest).toHaveBeenLastCalledWith({
      project_id: "project-1",
      objective: "Add a CSV export endpoint.",
      execution_profile_id: "profile-1",
      mode: "REVIEW_PLAN",
      idempotency_key: "idem-test",
      authorized_micros: 5_000_000,
    });
  });
});
