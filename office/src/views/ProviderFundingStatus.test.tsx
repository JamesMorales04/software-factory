import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { ProviderFundingStatus } from "./ProviderFundingStatus";

const mocks = vi.hoisted(() => ({
  getSubscriptionFunding: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
}));

const status = {
  scope: "UNSCOPED_PROVIDER_STATUS" as const,
  requestScoped: false as const,
  provider: "opencode-go",
  kind: "opencode",
  route: "GO_SUBSCRIPTION_INCLUDED",
  configured: true,
  enabled: true,
  model: "deepseek-v4.1-flash",
  settingsRevision: 7,
  authenticationClass: "API_KEY",
  coreVerifiable: false as const,
  admissible: false as const,
  entitlement: "NOT_RECORDED",
  extraUsage: "OWNER_ASSUMED_DISABLED_PENDING_ADMISSION",
  balanceFallback: "NOT_RECORDED",
  noChargeRoute: "EXACT_SUBSCRIPTION_ROUTE_ONLY",
  quota: {
    state: "UNKNOWN",
    reason: "NO_PROVIDER_QUOTA_OBSERVATION_REQUIRED",
  },
  missingEvidence: [
    "OWNER_CONFIRMED_EXTRA_USAGE_ASSUMPTION",
    "EXPLICIT_BOUNDED_SUBSCRIPTION_ADMISSION",
  ],
  note: "UNSCOPED PROVIDER STATUS: this projection cannot decide a request or Campaign.",
};

function renderStatus(provider = "opencode-go") {
  return render(
    <I18nProvider initialLocale="en">
      <ProviderFundingStatus provider={provider} />
    </I18nProvider>,
  );
}

describe("ProviderFundingStatus", () => {
  beforeEach(() => {
    mocks.getSubscriptionFunding.mockReset().mockResolvedValue(status);
  });

  it("shows the unscoped status without a verified or admissible claim", async () => {
    renderStatus();
    expect(await screen.findByText("UNSCOPED_PROVIDER_STATUS")).toBeDefined();
    expect(screen.getByText("NOT VERIFIED")).toBeDefined();
    expect(screen.getByText("Not satisfied")).toBeDefined();
    expect(
      screen.getByText("OWNER_ASSUMED_DISABLED_PENDING_ADMISSION"),
    ).toBeDefined();
    expect(
      screen.getByText(/cannot decide a request or Campaign/),
    ).toBeDefined();
    expect(mocks.getSubscriptionFunding).toHaveBeenCalledWith("opencode-go");
  });

  it("does not query non-subscription providers", () => {
    renderStatus("fixture");
    expect(mocks.getSubscriptionFunding).not.toHaveBeenCalled();
    expect(screen.queryByText("UNSCOPED_PROVIDER_STATUS")).toBeNull();
  });

  it("renders an unavailable state without inventing funding", async () => {
    mocks.getSubscriptionFunding.mockRejectedValue(new Error("offline"));
    renderStatus();
    expect(
      await screen.findByText(
        /funding status is unavailable from this Core build/,
      ),
    ).toBeDefined();
    expect(screen.queryByText("UNSCOPED_PROVIDER_STATUS")).toBeNull();
  });
});
