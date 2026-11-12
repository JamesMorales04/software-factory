import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type {
  CampaignDetail as CampaignDetailData,
  Gate,
  LeadRequestProjection,
  QualityReceipt,
} from "../../coreApi";
import { CampaignPulse } from "./CampaignPulse";

const task = (id: string, status: string) => ({
  task_id: id,
  campaign_id: "campaign-1",
  request_id: "request-1",
  unit_id: `unit-${id}`,
  sequence: Number(id.slice(-1)),
  status,
  base_commit: "a".repeat(40),
  depends_on: [],
  run_id: null,
  attempt_id: null,
  candidate_id: null,
  candidate_commit: null,
  candidate_tree: null,
  updated_at: "2026-11-06T10:00:00Z",
});

const detail: CampaignDetailData = {
  campaign: {
    id: "campaign-1",
    action_id: "action-1",
    status: "AUTHORIZED",
    created_at: "2026-11-06T09:00:00Z",
  },
  authorization: null,
  tasks: [task("task-1", "SUCCEEDED"), task("task-2", "RUNNING")],
  runs: [
    {
      id: "run-1",
      campaign_id: "campaign-1",
      status: "RUNNING",
      created_at: "2026-11-06T09:30:00Z",
    },
  ],
  attempts: [],
  candidates: [
    {
      id: "candidate-1",
      attempt_id: "attempt-1",
      action_spec_sha256: "b".repeat(64),
      commit_sha: "c".repeat(40),
      tree_sha: "d".repeat(40),
      implementer_id: "implementer-1",
      created_at: "2026-11-06T09:45:00Z",
      final_source: "MODEL_FINAL",
    },
  ],
  events: [
    {
      sequence: 1,
      subject_id: "task-1",
      kind: "task.succeeded",
      detail: "First task completed.",
      recorded_at: "2026-11-06T09:20:00Z",
    },
    {
      sequence: 2,
      subject_id: "run-1",
      kind: "run.started",
      detail: "Implementation run started.",
      recorded_at: "2026-11-06T09:30:00Z",
    },
    {
      sequence: 3,
      subject_id: "candidate-1",
      kind: "candidate.created",
      detail: "Candidate created by the implementer.",
      recorded_at: "2026-11-06T09:45:00Z",
    },
  ],
};

const gate: Gate = {
  candidate_id: "candidate-1",
  outcome: "ACCEPTED",
  reasons: [],
  decided_at: "2026-11-06T10:00:00Z",
};

const receipt = (
  role: "verifier" | "reviewer",
  result: string,
): QualityReceipt =>
  ({
    receipt_id: `${role}-receipt`,
    candidate_id: "candidate-1",
    role,
    result,
    received_at: "2026-11-06T09:55:00Z",
  }) as unknown as QualityReceipt;

const lead = {
  request: { state: "AWAITING_APPROVAL" },
  assessment: { id: "assessment-1" },
} as unknown as LeadRequestProjection;

describe("CampaignPulse", () => {
  it("renders only recorded task progress, receipts, gate and activity", () => {
    render(
      <CampaignPulse
        detail={detail}
        gates={{ "candidate-1": gate }}
        receipts={{
          "candidate-1": [
            receipt("verifier", "PASS"),
            receipt("reviewer", "CHANGES_REQUESTED"),
          ],
        }}
        sessions={[]}
        lead={lead}
      />,
    );

    const progress = screen.getByRole("progressbar", {
      name: "Overall progress",
    });
    expect(progress.getAttribute("aria-valuenow")).toBe("50");
    expect(screen.getByText("1 of 2 completed")).toBeDefined();
    expect(screen.getByText("Accepted")).toBeDefined();
    expect(screen.getByText("Changes requested")).toBeDefined();
    expect(screen.getByText("Passed")).toBeDefined();
    const leadRow = screen.getByText("Coordinating tasks").closest("li");
    expect(leadRow).not.toBeNull();
    expect(within(leadRow as HTMLElement).getByText("Completed")).toBeDefined();

    const activity = screen.getByRole("list", { name: "Recent activity" });
    const entries = within(activity).getAllByRole("listitem");
    expect(entries).toHaveLength(3);
    expect(entries[0]?.textContent).toContain("candidate.created");
    expect(entries[2]?.textContent).toContain("task.succeeded");
  });

  it("shows explicit pending states instead of inventing progress", () => {
    render(
      <CampaignPulse
        detail={{ ...detail, tasks: [], candidates: [], events: [] }}
        gates={{}}
        receipts={{}}
        sessions={[]}
        lead={null}
      />,
    );

    expect(
      screen
        .getByRole("progressbar", { name: "Overall progress" })
        .getAttribute("aria-valuenow"),
    ).toBe("0");
    expect(screen.getByText("Not started")).toBeDefined();
    expect(screen.getByText("No gate decision recorded yet.")).toBeDefined();
    expect(
      screen.getByText("No recorded activity in this Campaign."),
    ).toBeDefined();
  });
});
