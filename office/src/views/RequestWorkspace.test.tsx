import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CoreApiError,
  type FrozenLeadPlanRecord,
  type IntakeRequestProjection,
  type LeadAssessmentView,
  type LeadPlanProposalView,
  type SpecCheck,
} from "../coreApi";
import { RequestWorkspace } from "./RequestWorkspace";

const mocks = vi.hoisted(() => ({
  getIntakeRequest: vi.fn(),
  assessIntakeRequest: vi.fn(),
  approveIntakeRequest: vi.fn(),
  messageIntakeRequest: vi.fn(),
  cancelIntakeRequest: vi.fn(),
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

function renderRequest() {
  return render(
    <MemoryRouter initialEntries={["/requests/request-1"]}>
      <Routes>
        <Route path="/requests/:requestId" element={<RequestWorkspace />} />
        <Route path="/" element={<div>Home probe</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("RequestWorkspace", () => {
  beforeEach(() => {
    mocks.getIntakeRequest.mockReset().mockResolvedValue(awaiting);
    mocks.assessIntakeRequest.mockReset().mockResolvedValue(awaiting);
    mocks.approveIntakeRequest.mockReset().mockResolvedValue(awaiting);
    mocks.messageIntakeRequest.mockReset().mockResolvedValue(awaiting);
    mocks.cancelIntakeRequest.mockReset().mockResolvedValue(awaiting);
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
});
