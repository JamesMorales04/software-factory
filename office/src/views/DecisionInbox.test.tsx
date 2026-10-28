import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CoreApiError, type DecisionItem } from "../coreApi";
import { DecisionInbox } from "./DecisionInbox";

const mocks = vi.hoisted(() => ({
  getDecisions: vi.fn(),
  resolveModelCall: vi.fn(),
  approveIntakeRequest: vi.fn(),
  cancelIntakeRequest: vi.fn(),
  withdrawLeadRequest: vi.fn(),
  cancelCampaign: vi.fn(),
}));

vi.mock("../coreApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../coreApi")>();
  return {
    ...actual,
    coreApi: mocks,
  };
});

const PLAN_HASH = "a".repeat(64);
const PROFILE_HASH = "b".repeat(64);
const PROFILE_REVISION_ID = "profile-revision-7";

function intakeDecision(overrides: Partial<DecisionItem> = {}): DecisionItem {
  return {
    id: "intake-approval:intake-1",
    kind: "intake.plan-approval",
    severity: "attention",
    actionable: true,
    title: "Frozen plan awaiting approval",
    detail: "The frozen plan is ready for exact operator approval.",
    target: "/requests/intake-1",
    subject_id: "intake-1",
    updated_at: "2026-10-28T10:00:00Z",
    revision: "revision-1",
    project: {
      id: "project-1",
      repository: "https://example.test/repository.git",
      identity: "example",
    },
    campaign: null,
    change: {
      summary: "Implement the bounded intake endpoint.",
      work_units: 2,
      paths: ["src"],
      acceptance_criteria: 3,
      negative_cases: 1,
    },
    risk: { class: "R0", summary: "Frozen plan risk class R0." },
    impact: {
      budget: {
        max_attempts: 1,
        max_sessions: 1,
        max_turns: 2,
        max_input_tokens: 4000,
        max_output_tokens: 1000,
        max_cost_micros: 1000,
        deadline_secs: 300,
      },
      permissions: {
        execution_mode: "FIXTURE",
        allowed_paths: ["src"],
        allowed_tools: [],
        delegation_id: "delegation-1",
      },
      note: "Approval starts a Campaign under the exact frozen plan and budget.",
    },
    evidence: [
      { label: "Frozen plan", value: PLAN_HASH },
      { label: "Profile revision", value: PROFILE_REVISION_ID },
    ],
    blocked_reason: null,
    actions: [
      {
        kind: "approve_intake_plan",
        label: "Approve frozen plan",
        intake_id: "intake-1",
        expected_plan_hash: PLAN_HASH,
        expected_profile_revision_id: PROFILE_REVISION_ID,
        expected_profile_hash: PROFILE_HASH,
      },
      {
        kind: "cancel_intake",
        label: "Cancel request",
        intake_id: "intake-1",
      },
    ],
    ...overrides,
  };
}

function modelCallDecision(): DecisionItem {
  return {
    id: "model-call:reservation-1",
    kind: "model.usage-unresolved",
    severity: "critical",
    actionable: true,
    title: "Unresolved model reservation",
    detail: "The reservation remains unavailable for reuse.",
    target: "/runtime",
    subject_id: "reservation-1",
    updated_at: "2026-10-28T10:00:00Z",
    revision: "model-revision-1",
    project: { id: "project-1", repository: "repo" },
    change: { summary: "Reservation for Lead assessment." },
    actions: [
      {
        kind: "resolve_model_call",
        label: "Release reservation",
        model_call_id: "reservation-1",
      },
    ],
  };
}

function awarenessDecision(): DecisionItem {
  return {
    id: "run:run-1",
    kind: "run.failed",
    severity: "attention",
    actionable: false,
    title: "Run failed",
    detail: "Run run-1 for Campaign campaign-1 is FAILED.",
    target: "/campaigns/campaign-1",
    subject_id: "run-1",
    updated_at: "2026-10-28T09:00:00Z",
    revision: "run-revision-1",
    project: { id: "project-1", repository: "repo" },
    campaign: {
      id: "campaign-1",
      status: "FAILED",
      target: "/campaigns/campaign-1",
    },
    actions: [],
  };
}

function renderInbox() {
  return render(
    <MemoryRouter>
      <DecisionInbox />
    </MemoryRouter>,
  );
}

describe("Decision Inbox", () => {
  beforeEach(() => {
    try {
      window.localStorage.clear();
    } catch {
      // The locale defaults to English when storage is unavailable.
    }
    document.documentElement.lang = "en";
    mocks.getDecisions.mockReset().mockResolvedValue({
      items: [intakeDecision()],
      as_of: "2026-10-28T10:00:01Z",
    });
    mocks.resolveModelCall.mockReset().mockResolvedValue(undefined);
    mocks.approveIntakeRequest.mockReset().mockResolvedValue({});
    mocks.cancelIntakeRequest.mockReset().mockResolvedValue({});
    mocks.withdrawLeadRequest.mockReset().mockResolvedValue({});
    mocks.cancelCampaign.mockReset().mockResolvedValue({});
  });

  it("separates action-required items from informational events", async () => {
    mocks.getDecisions.mockResolvedValue({
      items: [intakeDecision(), modelCallDecision(), awarenessDecision()],
      as_of: "2026-10-28T10:00:01Z",
    });
    renderInbox();

    await screen.findByRole("heading", { name: "Decision Inbox" });
    expect(screen.getByText("Needs action")).toBeDefined();
    expect(screen.getByText("For awareness")).toBeDefined();
    expect(screen.getByText("2 actionable")).toBeDefined();
    expect(screen.getByText("1 for awareness")).toBeDefined();

    const awareness = screen.getByTestId("decision-item-run:run-1");
    expect(within(awareness).queryByRole("button")).toBeNull();
    expect(within(awareness).getByText("Run failed")).toBeDefined();

    const intake = screen.getByTestId("decision-item-intake-approval:intake-1");
    expect(within(intake).getByText("R0")).toBeDefined();
    expect(
      within(intake).getByText("Implement the bounded intake endpoint."),
    ).toBeDefined();
    expect(within(intake).getByText(PLAN_HASH)).toBeDefined();
  });

  it("approves a frozen intake plan only with the exact plan hash, profile revision and confirmation", async () => {
    const user = userEvent.setup();
    renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });

    await user.click(
      screen.getByRole("button", { name: /Approve frozen plan/ }),
    );
    const rationale = screen.getByLabelText("Decision rationale");
    const confirm = screen.getByRole("checkbox", {
      name: /I reviewed the frozen plan/,
    });
    const submit = screen.getByRole("button", {
      name: "Approve and start Campaign",
    });
    expect(submit.hasAttribute("disabled")).toBe(true);
    await user.type(rationale, "Reviewed the exact frozen plan and budget.");
    expect(submit.hasAttribute("disabled")).toBe(true);
    await user.click(confirm);
    expect(submit.hasAttribute("disabled")).toBe(false);

    await user.click(submit);

    await waitFor(() =>
      expect(mocks.approveIntakeRequest).toHaveBeenCalledWith("intake-1", {
        expected_plan_hash: PLAN_HASH,
        expected_profile_revision_id: PROFILE_REVISION_ID,
        expected_profile_hash: PROFILE_HASH,
        rationale: "Reviewed the exact frozen plan and budget.",
      }),
    );
    expect(
      await screen.findByText(
        "Core approved the exact frozen plan and started the Campaign.",
      ),
    ).toBeDefined();
  });

  it("revalidates the Core revision before submitting and aborts a stale item", async () => {
    const user = userEvent.setup();
    const changed = intakeDecision({
      revision: "revision-2",
      detail: "Core state changed after another operator action.",
    });
    mocks.getDecisions
      .mockResolvedValueOnce({
        items: [intakeDecision()],
        as_of: "2026-10-28T10:00:01Z",
      })
      .mockResolvedValue({
        items: [changed],
        as_of: "2026-10-28T10:00:05Z",
      });
    renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });

    await user.click(
      screen.getByRole("button", { name: /Approve frozen plan/ }),
    );
    await user.type(
      screen.getByLabelText("Decision rationale"),
      "This confirmation must not be posted.",
    );
    await user.click(
      screen.getByRole("checkbox", { name: /I reviewed the frozen plan/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "Approve and start Campaign" }),
    );

    expect(mocks.approveIntakeRequest).not.toHaveBeenCalled();
    expect(
      (await screen.findAllByText(/Core state changed since this item loaded/))
        .length,
    ).toBeGreaterThan(0);
    expect(mocks.getDecisions).toHaveBeenCalledTimes(2);
  });

  it("guards against double submission while the Core call is in flight", async () => {
    const user = userEvent.setup();
    let resolveApproval: () => void = () => undefined;
    const pending = new Promise<void>((resolve) => {
      resolveApproval = resolve;
    });
    mocks.approveIntakeRequest.mockReturnValue(pending);
    renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });

    await user.click(
      screen.getByRole("button", { name: /Approve frozen plan/ }),
    );
    await user.type(
      screen.getByLabelText("Decision rationale"),
      "One exact approval.",
    );
    await user.click(
      screen.getByRole("checkbox", { name: /I reviewed the frozen plan/ }),
    );
    const submit = screen.getByRole("button", {
      name: "Approve and start Campaign",
    });
    await user.click(submit);
    await user.click(submit);

    expect(mocks.approveIntakeRequest).toHaveBeenCalledTimes(1);
    resolveApproval();
    expect(
      await screen.findByText(
        "Core approved the exact frozen plan and started the Campaign.",
      ),
    ).toBeDefined();
  });

  it("surfaces an unauthorized Core decision without claiming success", async () => {
    const user = userEvent.setup();
    mocks.approveIntakeRequest.mockRejectedValue(
      new CoreApiError(401, "unauthorized"),
    );
    renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });

    await user.click(
      screen.getByRole("button", { name: /Approve frozen plan/ }),
    );
    await user.type(
      screen.getByLabelText("Decision rationale"),
      "Attempted approval.",
    );
    await user.click(
      screen.getByRole("checkbox", { name: /I reviewed the frozen plan/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "Approve and start Campaign" }),
    );

    expect(
      await screen.findByText(
        "This device is not authorized for that Core operation. Unlock or re-enroll this device.",
      ),
    ).toBeDefined();
    expect(
      screen.queryByText(
        "Core approved the exact frozen plan and started the Campaign.",
      ),
    ).toBeNull();
  });

  it("treats a Core stale-profile refusal as a stale item, not success", async () => {
    const user = userEvent.setup();
    mocks.approveIntakeRequest.mockRejectedValue(
      new CoreApiError(409, "conflict", {
        code: "PROFILE_REVISION_MISMATCH",
      }),
    );
    renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });

    await user.click(
      screen.getByRole("button", { name: /Approve frozen plan/ }),
    );
    await user.type(
      screen.getByLabelText("Decision rationale"),
      "Stale approval.",
    );
    await user.click(
      screen.getByRole("checkbox", { name: /I reviewed the frozen plan/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "Approve and start Campaign" }),
    );

    expect(
      (await screen.findAllByText(/Core state changed since this item loaded/))
        .length,
    ).toBeGreaterThan(0);
    expect(
      screen.queryByText(
        "Core approved the exact frozen plan and started the Campaign.",
      ),
    ).toBeNull();
  });

  it("goes offline without submitting when Core becomes unreachable", async () => {
    const user = userEvent.setup();
    mocks.getDecisions
      .mockResolvedValueOnce({
        items: [intakeDecision()],
        as_of: "2026-10-28T10:00:01Z",
      })
      .mockRejectedValue(new TypeError("Failed to fetch"));
    renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });

    await user.click(
      screen.getByRole("button", { name: /Approve frozen plan/ }),
    );
    await user.type(
      screen.getByLabelText("Decision rationale"),
      "Offline attempt.",
    );
    await user.click(
      screen.getByRole("checkbox", { name: /I reviewed the frozen plan/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "Approve and start Campaign" }),
    );

    expect(mocks.approveIntakeRequest).not.toHaveBeenCalled();
    expect(
      await screen.findByText("Factory Core is unreachable"),
    ).toBeDefined();
    expect(
      screen
        .getByRole("button", { name: "Approve and start Campaign" })
        .hasAttribute("disabled"),
    ).toBe(true);
  });

  it("resolves an ambiguous model call through the existing Core operation", async () => {
    const user = userEvent.setup();
    mocks.getDecisions.mockResolvedValue({
      items: [modelCallDecision()],
      as_of: "2026-10-28T10:00:01Z",
    });
    renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });

    const reason = screen.getByLabelText("Release reason");
    const release = screen.getByRole("button", { name: "Release reservation" });
    expect(release.hasAttribute("disabled")).toBe(true);
    await user.type(reason, "Provider aborted before usage was reported.");
    await user.click(release);

    await waitFor(() =>
      expect(mocks.resolveModelCall).toHaveBeenCalledWith(
        "reservation-1",
        "office-operator",
        "Provider aborted before usage was reported.",
      ),
    );
  });

  it("requires a reason and confirmation for an authoritative Lead withdrawal", async () => {
    const user = userEvent.setup();
    const lead: DecisionItem = {
      id: "lead-ready:lead-1",
      kind: "lead.ready",
      severity: "attention",
      actionable: true,
      title: "Frozen Lead plan ready",
      detail: "The frozen plan requires a start decision in Lead.",
      target: "/lead",
      subject_id: "lead-1",
      updated_at: "2026-10-28T10:00:00Z",
      revision: "lead-revision-1",
      actions: [
        {
          kind: "withdraw_lead",
          label: "Withdraw request",
          lead_request_id: "lead-1",
        },
      ],
    };
    mocks.getDecisions.mockResolvedValue({
      items: [lead],
      as_of: "2026-10-28T10:00:01Z",
    });
    renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });

    await user.click(screen.getByRole("button", { name: "Withdraw request…" }));
    const submit = screen.getByRole("button", { name: "Withdraw request" });
    expect(submit.hasAttribute("disabled")).toBe(true);
    await user.type(
      screen.getByLabelText("Reason"),
      "Superseded by a new request.",
    );
    await user.click(
      screen.getByRole("checkbox", {
        name: /I understand this withdraws the request/,
      }),
    );
    await user.click(submit);

    await waitFor(() =>
      expect(mocks.withdrawLeadRequest).toHaveBeenCalledWith(
        "lead-1",
        "Superseded by a new request.",
      ),
    );
  });
  it("offers exact Campaign cancellation but no blind full-scope authorization", async () => {
    const user = userEvent.setup();
    const campaign: DecisionItem = {
      id: "campaign-authorization:campaign-1",
      kind: "campaign.authorization",
      severity: "attention",
      actionable: true,
      title: "Campaign awaiting authorization",
      detail:
        "Full-scope authorization must be reviewed in the Campaign workspace.",
      target: "/campaigns/campaign-1",
      subject_id: "campaign-1",
      updated_at: "2026-10-28T10:00:00Z",
      revision: "campaign-revision-1",
      project: { id: "project-1", repository: "repo" },
      campaign: {
        id: "campaign-1",
        status: "BLOCKED_AUTHORIZATION",
        target: "/campaigns/campaign-1",
      },
      blocked_reason: "FULL_SCOPE_AUTHORIZATION_REQUIRED",
      actions: [
        {
          kind: "cancel_campaign",
          label: "Cancel Campaign",
          campaign_id: "campaign-1",
        },
      ],
    };
    mocks.getDecisions.mockResolvedValue({
      items: [campaign],
      as_of: "2026-10-28T10:00:01Z",
    });
    renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });

    expect(screen.queryByRole("button", { name: /Authorize/ })).toBeNull();
    expect(
      screen.getByRole("link", { name: "Open Campaign" }).getAttribute("href"),
    ).toBe("/campaigns/campaign-1");

    await user.click(screen.getByRole("button", { name: "Cancel Campaign…" }));
    await user.type(
      screen.getByLabelText("Reason"),
      "Superseded by a new scope.",
    );
    await user.click(
      screen.getByRole("checkbox", {
        name: /I understand cancellation stops the Campaign/,
      }),
    );
    await user.click(screen.getByRole("button", { name: "Cancel Campaign" }));

    await waitFor(() =>
      expect(mocks.cancelCampaign).toHaveBeenCalledWith(
        "campaign-1",
        "Superseded by a new scope.",
      ),
    );
  });
  it("renders deliberate empty and unauthorized states from Core", async () => {
    mocks.getDecisions.mockResolvedValue({
      items: [],
      as_of: "2026-10-28T10:00:01Z",
    });
    const first = renderInbox();
    await screen.findByRole("heading", { name: "Decision Inbox" });
    expect(screen.getByText("Nothing needs action")).toBeDefined();
    expect(screen.getByText("No unresolved background work")).toBeDefined();
    first.unmount();

    mocks.getDecisions.mockRejectedValue(new CoreApiError(401, "unauthorized"));
    renderInbox();
    expect(
      await screen.findByText(
        "This device session is not authorized to read decisions. Re-enroll or unlock this device.",
      ),
    ).toBeDefined();
    expect(screen.queryByText("Needs action")).toBeNull();
  });
});
