import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { CheapOpsPanel } from "./CheapOpsPanel";

const mocks = vi.hoisted(() => ({
  getCandidateInterpretation: vi.fn(),
}));

vi.mock("../../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    readonly status: number;
    constructor(status: number, message: string) {
      super(message);
      this.name = "CoreApiError";
      this.status = status;
    }
  },
}));

function renderPanel() {
  return render(
    <I18nProvider initialLocale="en">
      <CheapOpsPanel candidateId="candidate-1" />
    </I18nProvider>,
  );
}

describe("CheapOpsPanel", () => {
  beforeEach(() => {
    mocks.getCandidateInterpretation.mockReset().mockResolvedValue({
      candidateId: "candidate-1",
      reviewerReceiptIds: ["review-1", "review-2"],
      report: {
        authority: "OBSERVATION_ONLY",
        summary:
          "3 receipt(s): 3 pass, 0 fail/change, 0 not tested; 2 reviewer(s); verifier recorded.",
        passed: 3,
        failed: 0,
        notTested: 0,
        reviewerCount: 2,
        verifierPresent: true,
        gateOutcome: "ACCEPTED",
        gateReasons: [],
        notes: ["2 independent reviewer receipts are aggregated by the Gate"],
      },
    });
  });

  it("shows observation-only evidence without claiming authority", async () => {
    renderPanel();
    await userEvent.click(
      screen.getByRole("button", { name: "Interpret evidence" }),
    );
    expect(mocks.getCandidateInterpretation).toHaveBeenCalledWith(
      "candidate-1",
    );
    expect(await screen.findByText(/3 receipt\(s\)/)).toBeDefined();
    expect(screen.getByText("OBSERVATION_ONLY")).toBeDefined();
    expect(screen.getByText("ACCEPTED")).toBeDefined();
    expect(screen.getByText(/2 independent reviewer receipts/)).toBeDefined();
  });
});
