import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgenticCharterRecord, AgenticOwnerDecision } from "../coreApi";
import { I18nProvider } from "../i18n";
import { Charter } from "./Charter";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  listAgenticCharters: vi.fn(),
  listAgenticDecisions: vi.fn(),
  createAgenticCharter: vi.fn(),
  revokeAgenticCharter: vi.fn(),
  resolveAgenticOwnerDecision: vi.fn(),
  createAgenticOwnerDecision: vi.fn(),
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
}));

const charter: AgenticCharterRecord = {
  id: "charter-1",
  owner: "operator",
  projectId: "project-1",
  domains: ["repository"],
  riskAppetite: "R0",
  executionAutonomy: true,
  repairAutonomy: true,
  deliveryAutonomy: false,
  policyAutonomy: false,
  reservedMatters: ["credentials", "delivery"],
  expiresAt: "2026-12-09T00:00:00Z",
  revokedAt: null,
  revocationReason: null,
  createdAt: "2026-11-09T00:00:00Z",
  updatedAt: "2026-11-09T00:00:00Z",
};

const decision: AgenticOwnerDecision = {
  id: "decision-1",
  projectId: "project-1",
  requestId: "request-1",
  campaignId: null,
  matter: "public_contracts",
  risk: "R2",
  status: "PENDING",
  options: ["APPROVE", "DENY"],
  evidence: { objective: "change the public API", risk: "R2" },
  recommendation: "DENY",
  deadline: "2026-11-10T00:00:00Z",
  selectedOption: null,
  rationale: null,
  resolvedBy: null,
  createdAt: "2026-11-09T00:00:00Z",
  updatedAt: "2026-11-09T00:00:00Z",
  resolvedAt: null,
};

function renderCharter() {
  return render(
    <I18nProvider initialLocale="en">
      <Charter />
    </I18nProvider>,
  );
}

describe("Charter", () => {
  beforeEach(() => {
    mocks.listProjects.mockReset().mockResolvedValue([
      {
        id: "project-1",
        repository: "/repo",
        base_commit: "a".repeat(40),
        created_at: "x",
        name: "calendar",
      },
    ]);
    mocks.listAgenticCharters
      .mockReset()
      .mockResolvedValue({ charters: [charter] });
    mocks.listAgenticDecisions
      .mockReset()
      .mockResolvedValue({ decisions: [decision] });
    mocks.createAgenticCharter.mockReset().mockResolvedValue(charter);
    mocks.revokeAgenticCharter.mockReset().mockResolvedValue({});
    mocks.resolveAgenticOwnerDecision.mockReset().mockResolvedValue({});
    mocks.createAgenticOwnerDecision.mockReset().mockResolvedValue({});
  });

  it("shows the active charter and pending reserved decision", async () => {
    renderCharter();
    expect(
      (await screen.findAllByText("public_contracts")).length,
    ).toBeGreaterThan(0);
    expect(screen.getByText(/change the public API/)).toBeDefined();
    expect(screen.getByText(/Domains:/)).toBeDefined();
    expect(
      screen.getByText(/Reserved matters: credentials, delivery/),
    ).toBeDefined();
  });

  it("creates a charter with the selected bounds", async () => {
    mocks.listAgenticCharters.mockResolvedValue({ charters: [] });
    mocks.listAgenticDecisions.mockResolvedValue({ decisions: [] });
    renderCharter();
    await screen.findByText("No owner charter");
    await userEvent.click(
      screen.getByRole("checkbox", { name: "Delegate execution autonomy" }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Create charter" }),
    );
    expect(mocks.createAgenticCharter).toHaveBeenCalledWith(
      expect.objectContaining({
        project_id: "project-1",
        risk_appetite: "R0",
        execution_autonomy: true,
        repair_autonomy: false,
        delivery_autonomy: false,
      }),
    );
  });

  it("resolves a pending decision with an owner rationale", async () => {
    renderCharter();
    await screen.findAllByText("public_contracts");
    await userEvent.selectOptions(
      screen.getByLabelText("Owner option"),
      "APPROVE",
    );
    await userEvent.type(
      screen.getByLabelText("Owner rationale"),
      "Approved bounded public contract",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Resolve decision" }),
    );
    expect(mocks.resolveAgenticOwnerDecision).toHaveBeenCalledWith(
      "decision-1",
      "APPROVE",
      "Approved bounded public contract",
    );
  });

  it("opens a reserved decision for owner resolution", async () => {
    renderCharter();
    await screen.findAllByText("public_contracts");
    await userEvent.type(
      screen.getByLabelText("Decision context"),
      "Bounded credential rotation",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Open decision" }),
    );
    expect(mocks.createAgenticOwnerDecision).toHaveBeenCalledWith(
      expect.objectContaining({
        project_id: "project-1",
        matter: "credentials",
        risk: "R2",
        options: ["APPROVE", "DENY"],
        recommendation: "DENY",
      }),
    );
  });

  it("revokes a charter with a reason", async () => {
    renderCharter();
    await screen.findAllByText("public_contracts");
    await userEvent.type(
      screen.getByLabelText("Revocation reason"),
      "owner withdrew delegation",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Revoke charter" }),
    );
    expect(mocks.revokeAgenticCharter).toHaveBeenCalledWith(
      "charter-1",
      "owner withdrew delegation",
    );
  });
});
