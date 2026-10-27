import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CancelCampaign } from "./CancelCampaign";
import type { Campaign } from "./coreApi";

const mocks = vi.hoisted(() => ({
  cancelCampaign: vi.fn(),
}));

vi.mock("./coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
}));

const campaign: Campaign = {
  id: "campaign-1",
  action_id: "action-1",
  status: "AUTHORIZED",
  created_at: "2026-10-27T10:00:00Z",
};

describe("CancelCampaign", () => {
  beforeEach(() => {
    mocks.cancelCampaign.mockReset();
  });

  it("requires a consequence confirmation before calling Core", async () => {
    const onCancelled = vi.fn();
    mocks.cancelCampaign.mockResolvedValue({
      ...campaign,
      status: "CANCELLED",
    });
    render(<CancelCampaign campaign={campaign} onCancelled={onCancelled} />);
    await userEvent.click(
      screen.getByRole("button", { name: "Cancel Campaign" }),
    );
    expect(screen.getByText(/External provider termination/)).toBeDefined();
    await userEvent.type(
      screen.getByLabelText("Reason (optional)"),
      "operator cancellation",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Confirm cancellation" }),
    );
    expect(mocks.cancelCampaign).toHaveBeenCalledWith(
      campaign.id,
      "operator cancellation",
    );
    expect(onCancelled).toHaveBeenCalled();
  });

  it("does not offer cancellation for a terminal Campaign", () => {
    const { container } = render(
      <CancelCampaign
        campaign={{ ...campaign, status: "CANCELLED" }}
        onCancelled={vi.fn()}
      />,
    );
    expect(container.innerHTML).toBe("");
  });
});
