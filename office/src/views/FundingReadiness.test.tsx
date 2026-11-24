import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RequestFundingStatus } from "../coreApi";
import { I18nProvider } from "../i18n";
import { FundingReadinessPanel } from "./FundingReadiness";

const mocks = vi.hoisted(() => ({
  getIntakeFundingReadiness: vi.fn(),
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

const admitted: RequestFundingStatus = {
  schemaVersion: 1,
  scope: {
    intakeRequestId: "intake-1",
    leadRequestId: "lead-1",
    complete: true,
  },
  provider: "opencode-go",
  model: "deepseek-v4.1-flash",
  requestScoped: true,
  coreVerifiable: false,
  requestScopedAdmissible: true,
  verdict: "ADMITTED_FOR_REQUEST",
  admission: {
    admissionId: "admission-1",
    state: "ACTIVE",
    provider: "opencode-go",
    model: "deepseek-v4.1-flash",
    settingsRevision: "7",
    validUntilMs: 1_800_000_060_000,
    campaignId: null,
    extraUsage: "DISABLED",
    confirmedBy: "Product Owner",
    statement: "Extra Usage is disabled for this exact route.",
    ownerExtraUsageDeclarations: [
      {
        declarationId: "declaration-1",
        role: "lead",
        provider: "opencode-go",
        model: "deepseek-v4.1-flash",
        settingsRevision: "7",
      },
    ],
    routes: [
      {
        role: "lead",
        provider: "opencode-go",
        model: "deepseek-v4.1-flash",
        settingsRevision: "7",
        transport: "OPENCODE_GO_SUBSCRIPTION",
        valid: true,
      },
    ],
    receiptSha256: "a".repeat(64),
  },
  missingEvidence: [],
  recovery: "No recovery is required for this bounded admission.",
  provenance: [
    "REQUEST_SCOPED_SUBSCRIPTION_ADMISSION",
    "OWNER_ASSUMPTION_NOT_PROVIDER_EVIDENCE",
  ],
  note: "requestScopedAdmissible mirrors the existing Core scope-admission guard.",
};

function renderPanel() {
  return render(
    <I18nProvider initialLocale="en">
      <FundingReadinessPanel requestId="intake-1" />
    </I18nProvider>,
  );
}

describe("FundingReadinessPanel", () => {
  beforeEach(() => {
    mocks.getIntakeFundingReadiness.mockReset().mockResolvedValue(admitted);
  });

  it("renders the exact admission without claiming provider verification", async () => {
    renderPanel();
    expect(await screen.findByText("ADMITTED_FOR_REQUEST")).toBeDefined();
    expect(screen.getByText("admission-1")).toBeDefined();
    expect(screen.getByText("NOT VERIFIED")).toBeDefined();
    expect(screen.getByText("Satisfied for this exact scope")).toBeDefined();
    expect(screen.getByText("declaration-1")).toBeDefined();
    expect(
      screen.getByText(
        "No blocking evidence is missing for this exact admission.",
      ),
    ).toBeDefined();
    expect(
      screen.queryByText(/provider-issued entitlement is verified/i),
    ).toBeNull();
    expect(mocks.getIntakeFundingReadiness).toHaveBeenCalledWith("intake-1");
  });

  it("shows an explicit unscoped warning and never a green decision", async () => {
    mocks.getIntakeFundingReadiness.mockResolvedValue({
      ...admitted,
      scope: { intakeRequestId: "", leadRequestId: "", complete: false },
      requestScoped: false,
      requestScopedAdmissible: false,
      verdict: "SCOPE_REQUIRED",
      admission: null,
      missingEvidence: ["EXACT_REQUEST_SCOPE"],
    });
    renderPanel();
    expect(await screen.findByText("SCOPE_REQUIRED")).toBeDefined();
    expect(
      screen.getByText(
        /UNSCOPED STATUS: this response cannot decide a request/,
      ),
    ).toBeDefined();
    expect(screen.queryByText("ADMITTED_FOR_REQUEST")).toBeNull();
    expect(screen.queryByText("admission-1")).toBeNull();
  });

  it("shows unavailable when the Core build has no projection", async () => {
    const { CoreApiError } = await import("../coreApi");
    mocks.getIntakeFundingReadiness.mockRejectedValue(
      new CoreApiError(404, "not found"),
    );
    renderPanel();
    expect(
      await screen.findByText(
        /does not expose the request-scoped funding readiness/,
      ),
    ).toBeDefined();
  });

  it("shows a retryable read error without inventing readiness", async () => {
    const { CoreApiError } = await import("../coreApi");
    mocks.getIntakeFundingReadiness
      .mockRejectedValueOnce(new CoreApiError(503, "offline"))
      .mockResolvedValueOnce(admitted);
    renderPanel();
    expect(await screen.findByText("offline")).toBeDefined();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(mocks.getIntakeFundingReadiness).toHaveBeenCalledTimes(2),
    );
    expect(await screen.findByText("ADMITTED_FOR_REQUEST")).toBeDefined();
  });
});
