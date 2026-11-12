import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  type CampaignAuthorization,
  CoreApiError,
  type FrozenLeadPlanRecord,
  type IntakeRequestProjection,
  type LeadAssessmentView,
  type LeadPlanProposalView,
  type ParticipantPlanReadinessView,
  type PlanningReadinessView,
  type SpecCheck,
} from "../coreApi";
import { I18nProvider, type Locale } from "../i18n";
import { RequestWorkspace } from "./RequestWorkspace";

const mocks = vi.hoisted(() => ({
  getIntakeRequest: vi.fn(),
  assessIntakeRequest: vi.fn(),
  approveIntakeRequest: vi.fn(),
  messageIntakeRequest: vi.fn(),
  cancelIntakeRequest: vi.fn(),
  repairCampaign: vi.fn(),
  getRequestTrace: vi.fn(),
  getAgenticSpec: vi.fn(),
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
  newIdempotencyKey: () => "message-key",
}));

const check: SpecCheck = {
  id: "test",
  kind: "test",
  description: "Run the test suite",
  command: ["cargo", "test"],
  expectExit: 0,
  timeoutSecs: 120,
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
  knowledgeGaps: ["No compatibility test was supplied"],
  changeImpact: ["Adds a new endpoint"],
  validationNeeds: ["Integration test"],
  assumptions: ["Tests run in CI"],
  uncertainty: ["Export format details"],
  suggestedNextAction: "Freeze the proposed plan",
};

const proposal: LeadPlanProposalView = {
  projectId: "project-1",
  actionId: "action-1",
  baseRevision: "base-revision",
  projectConfigHash: "config-hash",
  objective: "Add a CSV export endpoint",
  class: "feature",
  assumptions: ["API stays backward compatible"],
  risks: ["Large report payloads"],
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
  workUnits: [
    {
      id: "unit-1",
      kind: "implement",
      description: "Implement the endpoint",
      dependsOn: [],
      allowedPaths: ["src"],
      capabilities: ["repository-edit"],
      acceptanceCriteria: [check],
      negativeCases: [],
    },
  ],
};

const plan: FrozenLeadPlanRecord = {
  id: "plan-1",
  request_id: "lead-1",
  proposal_id: "proposal-1",
  plan_hash: "plan-hash",
  config_hash: "config-hash",
  delegation_hash: "delegation-hash",
  created_at: "2026-10-27T10:02:00Z",
};

const readyParticipantPlan: ParticipantPlanReadinessView = {
  required: true,
  present: true,
  ready: true,
  reviewerCount: 5,
  requiredReviewerCount: 5,
  reviewerRoles: [
    "FUNCTIONAL",
    "ARCHITECTURE",
    "CODE_QUALITY",
    "SECURITY",
    "TESTING",
  ],
  requiredRoles: [
    "FUNCTIONAL",
    "ARCHITECTURE",
    "CODE_QUALITY",
    "SECURITY",
    "TESTING",
  ],
  missingRoles: [],
  unexpectedRoles: [],
  missingAssignments: [],
  graphStale: false,
  problems: [],
};

const readyPlanning: PlanningReadinessView = {
  lane: "FORMAL_SPEC",
  artifacts: [],
  review: null,
  workflow: null,
  missingArtifacts: [],
  participantPlan: readyParticipantPlan,
  reviewStatus: "AUTO_APPROVED",
  reviewApproved: true,
  materialsReady: true,
  ready: true,
};

function makeProjection(
  overrides: Partial<IntakeRequestProjection> = {},
): IntakeRequestProjection {
  return {
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
    assessment,
    proposal,
    plan,
    planning: readyPlanning,
    campaign: null,
    messages: [
      {
        id: "message-1",
        role: "operator",
        body: "Keep the API backward compatible.",
        createdAt: "2026-10-27T10:01:00Z",
      },
    ],
    createdAt: "2026-10-27T10:00:00Z",
    updatedAt: "2026-10-27T10:05:00Z",
    ...overrides,
  };
}

const submitted = makeProjection({
  state: "SUBMITTED",
  stateDetail: null,
  assessment: null,
  proposal: null,
  plan: null,
  messages: [],
});

const planReady = makeProjection({
  state: "PLAN_READY",
  stateDetail: "Proposal recorded; frozen plan pending",
  plan: null,
});

const awaiting = makeProjection();

function renderRequest(locale?: Locale) {
  const content = (
    <MemoryRouter initialEntries={["/requests/request-1"]}>
      <Routes>
        <Route path="/requests/:requestId" element={<RequestWorkspace />} />
        <Route path="/" element={<div>Home probe</div>} />
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

describe("RequestWorkspace", () => {
  beforeEach(() => {
    mocks.getIntakeRequest.mockReset().mockResolvedValue(awaiting);
    mocks.assessIntakeRequest.mockReset().mockResolvedValue(awaiting);
    mocks.approveIntakeRequest.mockReset().mockResolvedValue(awaiting);
    mocks.messageIntakeRequest.mockReset().mockResolvedValue(awaiting);
    mocks.cancelIntakeRequest.mockReset().mockResolvedValue(awaiting);
    mocks.getAgenticSpec.mockReset().mockResolvedValue({
      frozen: null,
      draft: null,
    });
    mocks.getRequestTrace.mockReset().mockResolvedValue({
      requestId: "request-1",
      run: null,
      trace: null,
      events: [],
      steps: [],
      alerts: [],
      terminal: false,
      eventLimit: 200,
      truncated: false,
      stepLimit: 500,
      stepsTruncated: false,
      replayError: null,
    });
    mocks.repairCampaign.mockReset().mockResolvedValue({
      requestId: "repair-1",
      projectId: "project-1",
      candidateId: "candidate-1",
      baseCommit: "c".repeat(40),
      profileId: "profile-1",
      mode: "AUTONOMOUS",
      grantDelegationId: "delegation-1",
      replayed: false,
    });
  });

  it("runs autonomous requests without manual assessment or approval controls", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({
        mode: "AUTONOMOUS",
        state: "AWAITING_APPROVAL",
      }),
    );
    renderRequest();
    expect(
      await screen.findByText(/Lead autopilot is assessing, freezing/),
    ).toBeDefined();
    expect(screen.queryByRole("button", { name: "Run assessment" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Approve frozen plan and start" }),
    ).toBeNull();
    expect(screen.queryByText("Subscription plan confirmation")).toBeNull();
  });

  it("renders the request workspace in Spanish and preserves machine identifiers", async () => {
    renderRequest("es");
    await screen.findByRole("heading", { name: "Evaluación" });
    expect(screen.getByRole("heading", { name: "Plan" })).toBeDefined();
    expect(screen.getByText("Pendiente de aprobación")).toBeDefined();
    expect(
      screen.getByLabelText("Justificación de la aprobación"),
    ).toBeDefined();
    expect(
      screen.getByRole("button", {
        name: "Aprobar el plan congelado e iniciar",
      }),
    ).toBeDefined();
    expect(
      screen.getByRole("heading", { name: "Detalles de la solicitud" }),
    ).toBeDefined();
    expect(screen.getAllByText("plan-hash").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Congelado").length).toBeGreaterThan(0);
    expect(screen.getByText("Unidades de trabajo")).toBeDefined();
  });

  it("renders the assessment, frozen plan, authorization state and messages", async () => {
    renderRequest();
    await screen.findByRole("heading", {
      name: "Add a CSV export endpoint",
    });
    expect(screen.getByText(/Add a supported export endpoint/)).toBeDefined();
    expect(screen.getAllByText("Frozen").length).toBeGreaterThan(0);
    expect(screen.getAllByText("plan-hash").length).toBeGreaterThan(0);
    expect(screen.queryByText("Not frozen")).toBeNull();
    expect(screen.getByText(/Authorization required/)).toBeDefined();
    expect(screen.getByText("Keep the API backward compatible.")).toBeDefined();
    expect(screen.queryByRole("link", { name: /Open Campaign/ })).toBeNull();
  });

  it("guards approval until a rationale and scope confirmation are present", async () => {
    renderRequest();
    const approve = await screen.findByRole("button", {
      name: "Approve frozen plan and start",
    });
    expect((approve as HTMLButtonElement).disabled).toBe(true);
    await userEvent.type(
      screen.getByLabelText("Approval rationale"),
      "Reviewed the frozen plan and budget.",
    );
    expect((approve as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: /I reviewed the frozen scope/,
      }),
    );
    expect((approve as HTMLButtonElement).disabled).toBe(false);
    await userEvent.click(approve);
    await waitFor(() =>
      expect(mocks.approveIntakeRequest).toHaveBeenCalledWith("request-1", {
        expected_plan_hash: "plan-hash",
        rationale: "Reviewed the frozen plan and budget.",
      }),
    );
    expect(await screen.findByText(/Core recorded the approval/)).toBeDefined();
  });

  it("shows an incomplete plan-review callout and blocks approval", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({
        planning: {
          ...readyPlanning,
          missingArtifacts: ["PLAN", "WORKGRAPH"],
          ready: false,
          participantPlan: {
            ...readyParticipantPlan,
            present: false,
            ready: false,
            reviewerCount: 0,
            reviewerRoles: [],
            missingRoles: readyParticipantPlan.requiredRoles,
            missingAssignments: readyParticipantPlan.requiredRoles,
          },
        },
      }),
    );
    renderRequest();
    expect(await screen.findByText("Plan review incomplete")).toBeDefined();
    expect(
      screen.getByText("Missing artifacts: PLAN, WORKGRAPH."),
    ).toBeDefined();
    expect(screen.getByText(/participant plan is not recorded/i)).toBeDefined();
    const approve = screen.getByRole("button", {
      name: "Approve frozen plan and start",
    });
    expect((approve as HTMLButtonElement).disabled).toBe(true);
    expect(
      (screen.getByLabelText("Approval rationale") as HTMLTextAreaElement)
        .disabled,
    ).toBe(true);
    expect(
      (
        screen.getByRole("checkbox", {
          name: /I reviewed the frozen scope/,
        }) as HTMLInputElement
      ).disabled,
    ).toBe(true);
    expect(
      screen
        .getByRole("link", { name: /Open Director plan review/ })
        .getAttribute("href"),
    ).toBe("/director?request=request-1");
  });

  it("blocks approval when the participant plan does not match the frozen reviewer plan", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({
        planning: {
          ...readyPlanning,
          ready: false,
          participantPlan: {
            ...readyParticipantPlan,
            ready: false,
            reviewerCount: 1,
            reviewerRoles: ["FUNCTIONAL"],
            missingRoles: [
              "ARCHITECTURE",
              "CODE_QUALITY",
              "SECURITY",
              "TESTING",
            ],
            missingAssignments: [
              "ARCHITECTURE",
              "CODE_QUALITY",
              "SECURITY",
              "TESTING",
            ],
            problems: [
              "the participant plan requires 1 reviewer(s); the frozen plan requires 5",
            ],
          },
        },
      }),
    );
    renderRequest();
    expect(
      await screen.findByText(
        /does not match the frozen reviewer plan: ARCHITECTURE, CODE_QUALITY, SECURITY, TESTING/,
      ),
    ).toBeDefined();
    expect(
      (
        screen.getByRole("button", {
          name: "Approve frozen plan and start",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
  });

  it("blocks approval while the plan review awaits the owner decision", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({
        planning: {
          ...readyPlanning,
          ready: false,
          reviewStatus: "PENDING",
          reviewApproved: false,
          materialsReady: true,
        },
      }),
    );
    renderRequest();
    expect(
      await screen.findByText(/plan review is awaiting the owner decision/i),
    ).toBeDefined();
    expect(
      (
        screen.getByRole("button", {
          name: "Approve frozen plan and start",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(
      (screen.getByLabelText("Approval rationale") as HTMLTextAreaElement)
        .disabled,
    ).toBe(true);
  });

  it("explains an unavailable authorization and disables approval", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({
        authorization: {
          required: true,
          available: false,
          reason: "No executable implementer route is configured.",
        },
      }),
    );
    renderRequest();
    expect(
      (
        await screen.findAllByText(
          /No executable implementer route is configured/,
        )
      ).length,
    ).toBeGreaterThan(0);
    const approve = screen.getByRole("button", {
      name: "Approve frozen plan and start",
    });
    expect((approve as HTMLButtonElement).disabled).toBe(true);
    expect(
      (screen.getByLabelText("Approval rationale") as HTMLTextAreaElement)
        .disabled,
    ).toBe(true);
    expect(
      (
        screen.getByRole("checkbox", {
          name: /I reviewed the frozen scope/,
        }) as HTMLInputElement
      ).disabled,
    ).toBe(true);
  });

  it("runs the assessment Core operation and refreshes the projection", async () => {
    mocks.getIntakeRequest
      .mockResolvedValueOnce(submitted)
      .mockResolvedValue(awaiting);
    renderRequest();
    await screen.findByText("No assessment recorded");
    await userEvent.click(
      screen.getByRole("button", { name: "Run assessment" }),
    );
    await waitFor(() =>
      expect(mocks.assessIntakeRequest).toHaveBeenCalledWith("request-1"),
    );
    expect(
      await screen.findByText(/Core recorded the assessment/),
    ).toBeDefined();
    expect((await screen.findAllByText("Frozen")).length).toBeGreaterThan(0);
  });

  it("completes the assessment and freezes the plan before approval", async () => {
    mocks.getIntakeRequest
      .mockResolvedValueOnce(planReady)
      .mockResolvedValue(awaiting);
    renderRequest();
    expect(await screen.findByText("Not frozen")).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", {
        name: "Complete assessment and freeze plan",
      }),
    );
    await waitFor(() =>
      expect(mocks.assessIntakeRequest).toHaveBeenCalledWith("request-1"),
    );
    expect(await screen.findByText(/Core froze the plan/)).toBeDefined();
    expect((await screen.findAllByText("Frozen")).length).toBeGreaterThan(0);
  });

  it("records a durable pre-freeze operator message", async () => {
    const preFreeze = makeProjection({
      state: "PLAN_READY",
      plan: null,
      messages: [],
    });
    const withMessage = makeProjection({
      state: "PLAN_READY",
      plan: null,
      messages: [
        {
          id: "message-2",
          role: "operator",
          body: "Please add a negative test.",
          createdAt: "2026-10-27T10:06:00Z",
        },
      ],
    });
    mocks.getIntakeRequest
      .mockResolvedValueOnce(preFreeze)
      .mockResolvedValue(withMessage);
    renderRequest();
    await screen.findByLabelText("Message the Factory");
    await userEvent.type(
      screen.getByLabelText("Message the Factory"),
      "Please add a negative test.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(mocks.messageIntakeRequest).toHaveBeenCalledWith("request-1", {
        body: "Please add a negative test.",
        idempotency_key: "message-key",
      }),
    );
    expect(
      await screen.findByText("Please add a negative test."),
    ).toBeDefined();
    expect(
      (screen.getByLabelText("Message the Factory") as HTMLTextAreaElement)
        .value,
    ).toBe("");
  });

  it("starts one follow-up from a completed request and exposes the created request", async () => {
    const completed = makeProjection({
      state: "COMPLETED",
      stateDetail: "Quality Gate recorded ACCEPTED for the exact candidate.",
    });
    const followUp = makeProjection({
      id: "request-2",
      state: "SUBMITTED",
      parentRequestId: "request-1",
      plan: null,
      assessment: null,
      proposal: null,
      messages: [],
    });
    const completedWithFollowUp = makeProjection({
      state: "COMPLETED",
      followUpRequest: followUp,
    });
    mocks.getIntakeRequest
      .mockResolvedValueOnce(completed)
      .mockResolvedValue(completedWithFollowUp);
    mocks.messageIntakeRequest.mockResolvedValue(completedWithFollowUp);
    renderRequest();
    await screen.findByLabelText("Start a follow-up request");
    await userEvent.type(
      screen.getByLabelText("Start a follow-up request"),
      "Add the CSV export follow-up.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Start follow-up" }),
    );
    await waitFor(() =>
      expect(mocks.messageIntakeRequest).toHaveBeenCalledWith("request-1", {
        body: "Add the CSV export follow-up.",
        idempotency_key: "message-key",
      }),
    );
    expect(
      await screen.findByText(/Core created the follow-up request with state/),
    ).toBeDefined();
    expect(
      screen.getByRole("link", { name: "Open follow-up" }).getAttribute("href"),
    ).toBe("/requests/request-2");
    expect(screen.queryByLabelText("Start a follow-up request")).toBeNull();
  });

  it("does not offer a follow-up on a withdrawn terminal request", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({
        state: "CANCELLED",
        authorization: {
          required: false,
          available: false,
          reason: "INTAKE_WITHDRAWN",
        },
      }),
    );
    renderRequest();
    await screen.findByText("Keep the API backward compatible.");
    expect(screen.queryByLabelText("Start a follow-up request")).toBeNull();
  });

  it("explains a follow-up policy refusal in English", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({ state: "COMPLETED" }),
    );
    mocks.messageIntakeRequest.mockRejectedValue(
      new CoreApiError(409, "policy no longer matches", {
        code: "FOLLOW_UP_EXECUTION_POLICY_REQUIRED",
        error: "policy no longer matches",
      }),
    );
    renderRequest();
    await screen.findByLabelText("Start a follow-up request");
    await userEvent.type(
      screen.getByLabelText("Start a follow-up request"),
      "Continue with a new objective.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Start follow-up" }),
    );
    expect(
      await screen.findByText(/parent owner-confirmed execution policy/),
    ).toBeDefined();
    expect(screen.getByRole("alert")).toBeDefined();
    expect(screen.getByLabelText("Start a follow-up request")).toBeDefined();
  });

  it("explains a follow-up policy refusal in Spanish", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({ state: "COMPLETED" }),
    );
    mocks.messageIntakeRequest.mockRejectedValue(
      new CoreApiError(409, "policy no longer matches", {
        code: "FOLLOW_UP_EXECUTION_POLICY_REQUIRED",
        error: "policy no longer matches",
      }),
    );
    renderRequest("es");
    await screen.findByLabelText("Iniciar una solicitud de seguimiento");
    await userEvent.type(
      screen.getByLabelText("Iniciar una solicitud de seguimiento"),
      "Continúa con un nuevo objetivo.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Iniciar seguimiento" }),
    );
    expect(
      await screen.findByText(
        /política de ejecución confirmada por el propietario/,
      ),
    ).toBeDefined();
    expect(screen.getByRole("alert")).toBeDefined();
  });

  it("explains a follow-up principal mismatch in English", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({ state: "COMPLETED" }),
    );
    mocks.messageIntakeRequest.mockRejectedValue(
      new CoreApiError(409, "principal mismatch", {
        code: "FOLLOW_UP_POLICY_PRINCIPAL_MISMATCH",
        error: "principal mismatch",
      }),
    );
    renderRequest();
    await screen.findByLabelText("Start a follow-up request");
    await userEvent.type(
      screen.getByLabelText("Start a follow-up request"),
      "Continue with another principal.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Start follow-up" }),
    );
    expect(
      await screen.findByText(
        /only the principal that confirmed the parent execution policy/,
      ),
    ).toBeDefined();
    expect(screen.getByRole("alert")).toBeDefined();
    expect(screen.getByLabelText("Start a follow-up request")).toBeDefined();
  });

  it("explains a follow-up principal mismatch in Spanish", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({ state: "COMPLETED" }),
    );
    mocks.messageIntakeRequest.mockRejectedValue(
      new CoreApiError(409, "principal mismatch", {
        code: "FOLLOW_UP_POLICY_PRINCIPAL_MISMATCH",
        error: "principal mismatch",
      }),
    );
    renderRequest("es");
    await screen.findByLabelText("Iniciar una solicitud de seguimiento");
    await userEvent.type(
      screen.getByLabelText("Iniciar una solicitud de seguimiento"),
      "Continúa con otro principal.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Iniciar seguimiento" }),
    );
    expect(
      await screen.findByText(
        /solo el principal que confirmó la política de ejecución del padre/,
      ),
    ).toBeDefined();
    expect(screen.getByRole("alert")).toBeDefined();
    expect(
      screen.getByLabelText("Iniciar una solicitud de seguimiento"),
    ).toBeDefined();
  });

  it("shows the parent thread on a follow-up request", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({
        id: "request-2",
        state: "SUBMITTED",
        parentRequestId: "request-1",
        plan: null,
        assessment: null,
        proposal: null,
      }),
    );
    const { container } = renderRequest();
    expect(await screen.findByText(/Follow-up from/)).toBeDefined();
    const parentLink = container.querySelector('a[href="/requests/request-1"]');
    expect(parentLink).not.toBeNull();
    expect(parentLink?.textContent).toContain("request-");
  });

  it("starts a bounded repair from the exact rejected candidate after confirmation", async () => {
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({
        campaignId: "campaign-1",
        campaignStatus: "AUTHORIZED",
        gateOutcome: "REPAIR_REQUIRED",
      }),
    );
    renderRequest();
    await screen.findByText("Keep the API backward compatible.");
    await userEvent.click(
      screen.getByRole("button", { name: "Repair this candidate…" }),
    );
    await userEvent.type(
      screen.getByLabelText("Operator context for the repair"),
      "Fix the date validator and keep the public API stable.",
    );
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: /I confirm the exact candidate/,
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Start repair" }));
    expect(mocks.repairCampaign).toHaveBeenCalledWith("campaign-1", {
      confirm: true,
      reason: "Fix the date validator and keep the public API stable.",
    });
  });

  it("does not accept amendments through the message composer after freeze", async () => {
    renderRequest();
    await screen.findByText("Keep the API backward compatible.");
    expect(screen.queryByLabelText("Message the Factory")).toBeNull();
    expect(screen.getByText(/The executable plan is frozen/)).toBeDefined();
  });

  it("surfaces the authoritative Campaign when cancellation is fenced", async () => {
    mocks.cancelIntakeRequest.mockRejectedValue(
      new CoreApiError(409, "lead request already started", {
        code: "LEAD_ALREADY_STARTED",
        campaign_id: "campaign-9",
        campaign_status: "AUTHORIZED",
      }),
    );
    renderRequest();
    await screen.findByText("Keep the API backward compatible.");
    await userEvent.click(
      screen.getByRole("button", { name: "Cancel this request…" }),
    );
    await userEvent.type(screen.getByLabelText("Cancellation reason"), "stop");
    await userEvent.click(
      screen.getByRole("button", { name: "Confirm cancellation" }),
    );
    expect(await screen.findByText(/A Campaign already started/)).toBeDefined();
    expect(
      screen.getByRole("link", { name: "Open Campaign" }).getAttribute("href"),
    ).toBe("/campaigns/campaign-9");
  });

  it("surfaces approval failure without clearing the operator rationale", async () => {
    mocks.approveIntakeRequest.mockRejectedValue(
      new CoreApiError(409, "execution profile revision is stale"),
    );
    renderRequest();
    await screen.findByRole("button", {
      name: "Approve frozen plan and start",
    });
    await userEvent.type(
      screen.getByLabelText("Approval rationale"),
      "Reviewed scope.",
    );
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: /I reviewed the frozen scope/,
      }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Approve frozen plan and start" }),
    );
    expect(
      await screen.findByText(/execution profile revision is stale/),
    ).toBeDefined();
    expect(
      (screen.getByLabelText("Approval rationale") as HTMLTextAreaElement)
        .value,
    ).toBe("Reviewed scope.");
    expect(screen.queryByText(/Core recorded the approval/)).toBeNull();
  });

  it("keeps the last projection and pauses actions when a refresh fails", async () => {
    mocks.getIntakeRequest
      .mockResolvedValueOnce(
        makeProjection({
          state: "SUBMITTED",
          stateDetail: null,
          assessment: null,
          proposal: null,
          plan: null,
        }),
      )
      .mockRejectedValueOnce(new Error("Core unreachable"));
    renderRequest();
    await screen.findByText("Keep the API backward compatible.");
    await userEvent.click(
      screen.getByRole("button", { name: "Run assessment" }),
    );
    expect(
      await screen.findByText(/Core could not be refreshed: Core unreachable/),
    ).toBeDefined();
    expect(screen.getByText("Keep the API backward compatible.")).toBeDefined();
    expect(
      (
        screen.getByRole("button", {
          name: "Run assessment",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(screen.getByRole("button", { name: "Retry now" })).toBeDefined();
  });

  it("distinguishes a missing request from a generic load failure", async () => {
    mocks.getIntakeRequest.mockRejectedValue(
      new CoreApiError(404, "intake request not found"),
    );
    renderRequest();
    expect(await screen.findByText("Request not found")).toBeDefined();
    expect(
      screen.getByRole("link", { name: "Back to Home" }).getAttribute("href"),
    ).toBe("/");
  });

  it("renders the camelCase V2 authorization risk, mode and route", async () => {
    const authorization: CampaignAuthorization = {
      campaignId: "campaign-1",
      authorizationId: "authorization-1",
      riskClass: "R0",
      executionMode: "TRUSTED_LOCAL_PROVIDER",
      actor: "fixture-owner",
      rationale: "Fixture V2 authorization.",
      authorizedPaths: ["src"],
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
      providerMode: "build",
      delegationId: "delegation-1",
      allowedTools: [],
      schemaVersion: 2,
      policy: {
        policyId: "policy-1",
        authorizationId: "authorization-1",
        monetaryEnvelopeId: "envelope-1",
        scopeSha256: "a".repeat(64),
        operations: {
          modelRequestTimeoutSecs: 300,
          toolCallTimeoutSecs: 600,
          maxRequestBytes: 2097152,
          maxResponseBytes: 2097152,
          maxGenerationTokens: 8192,
          maxInFlightOperations: 4,
        },
        progress: {
          diagnoseAfterStagnantCheckpoints: 2,
          escalateAfterUnsuccessfulReplans: 1,
          observationTimeoutSecs: 300,
        },
      },
      legacy: null,
      createdAt: "2026-10-29T10:00:00Z",
    };
    mocks.getIntakeRequest.mockResolvedValue(
      makeProjection({
        campaignId: "campaign-1",
        campaign: {
          campaign: {
            id: "campaign-1",
            action_id: "action-1",
            status: "AUTHORIZED",
            created_at: "2026-10-29T10:00:00Z",
          },
          authorization,
          tasks: [],
          runs: [],
          attempts: [],
          candidates: [],
          events: [],
        },
      }),
    );
    renderRequest();
    expect(
      await screen.findByText(
        "R0 · TRUSTED_LOCAL_PROVIDER · opencode-go/deepseek-v4.1-flash",
      ),
    ).toBeDefined();
    expect(screen.queryByText(/undefined/)).toBeNull();
  });
});
