import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ModelCallRecord } from "../coreApi";
import { Runtime } from "./Runtime";

const mocks = vi.hoisted(() => ({
  health: vi.fn(),
  listWorkers: vi.fn(),
  listProviderSessions: vi.fn(),
  listModelCalls: vi.fn(),
  listUnresolvedModelCalls: vi.fn(),
  getObservability: vi.fn(),
  getRuntimeSettings: vi.fn(),
  getProviderTurnBudget: vi.fn(),
  resolveModelCall: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
}));

const unresolvedCall: ModelCallRecord = {
  reservation_id: "reservation-1",
  role: "semantic_reviewer",
  principal_id: "reviewer",
  project_id: "project-1",
  objective_id: "candidate-1",
  scope_id: "candidate-1",
  provider: "opencode-go",
  model: "deepseek-v4.1-flash",
  purpose: "semantic_review",
  request_sha256: "a".repeat(64),
  input_tokens: 0,
  output_tokens: 0,
  total_tokens: 0,
  cost_micros: 0,
  max_cost_micros: 500000,
  usage_known: false,
  state: "AMBIGUOUS",
  error: "reservation expired before settlement",
  created_at: "2026-10-27T10:00:00Z",
  updated_at: "2026-10-27T10:01:00Z",
};

function renderRuntime(path = "/runtime") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Runtime />
    </MemoryRouter>,
  );
}

describe("Runtime", () => {
  beforeEach(() => {
    mocks.health.mockReset().mockResolvedValue({
      healthy: true,
      version: "0.1.0",
    });
    mocks.listWorkers
      .mockReset()
      .mockResolvedValue([
        { worker_id: "worker-1", updated_at: "2026-10-27T10:00:00Z" },
      ]);
    mocks.listProviderSessions.mockReset().mockResolvedValue([]);
    mocks.listModelCalls.mockReset().mockResolvedValue([unresolvedCall]);
    mocks.listUnresolvedModelCalls
      .mockReset()
      .mockResolvedValue([unresolvedCall]);
    mocks.getObservability.mockReset().mockResolvedValue({
      enabled: false,
      cursor: 0,
      exported: 0,
      dropped: 0,
      failed_attempts: 0,
      pending: 0,
      last_error: null,
    });
    mocks.getRuntimeSettings.mockReset().mockResolvedValue({
      semanticReviewEnabled: false,
      semanticReviewMaxOutputTokens: 8192,
      updatedAt: "2026-10-27T10:00:00Z",
    });
    mocks.getProviderTurnBudget.mockReset().mockResolvedValue({
      reserved_turns: 0,
      ceiling_micros: 0,
    });
    mocks.resolveModelCall.mockReset().mockResolvedValue(undefined);
  });

  it("separates measured, reserved and unresolved runtime state", async () => {
    renderRuntime();
    await screen.findByRole("heading", { name: "Runtime" });
    expect(screen.getByText("Reserved ceiling")).toBeDefined();
    expect(screen.getByText("Unresolved calls")).toBeDefined();
    expect(screen.getByText("Core healthy")).toBeDefined();
    expect(
      screen.getByText(/Supervisor and nested runtime state/),
    ).toBeDefined();

    await userEvent.click(screen.getByRole("tab", { name: /Sessions/ }));
    await screen.findByRole("heading", { name: "Provider sessions (0 shown)" });
    expect(screen.getByText("No matching sessions")).toBeDefined();
  });

  it("deep-links directly to Diagnostics", async () => {
    renderRuntime("/runtime?tab=diagnostics");
    await screen.findByRole("heading", { name: "Service boundaries" });
    expect(
      screen
        .getByRole("tab", { name: "Diagnostics" })
        .getAttribute("aria-selected"),
    ).toBe("true");
  });

  it("shows unresolved reservations and releases only after an explicit reason", async () => {
    renderRuntime("/runtime?tab=model-calls");
    await screen.findByRole("heading", {
      name: "Unresolved reservations (1)",
    });
    expect(
      screen.getByText("ceiling 500,000µ", { exact: false }),
    ).toBeDefined();
    const reason = screen.getByLabelText("Release reason");
    const release = screen.getByRole("button", { name: "Release reservation" });
    expect(release.hasAttribute("disabled")).toBe(true);
    await userEvent.type(reason, "reservation expired before settlement");
    await userEvent.click(release);
    await waitFor(() =>
      expect(mocks.resolveModelCall).toHaveBeenCalledWith(
        "reservation-1",
        "office-operator",
        "reservation expired before settlement",
      ),
    );
  });
});
