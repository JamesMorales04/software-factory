import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderView } from "../coreApi";
import { I18nProvider, type Locale } from "../i18n";
import { Settings } from "./Settings";

const mocks = vi.hoisted(() => ({
  listProviders: vi.fn(),
  getRuntimeSettings: vi.fn(),
  updateRuntimeSettings: vi.fn(),
  updateProvider: vi.fn(),
  testProvider: vi.fn(),
  listProviderModels: vi.fn(),
  revokeProviderCredential: vi.fn(),
  providerAudit: vi.fn(),
  listExecutionProfiles: vi.fn(),
  listProjects: vi.fn(),
  getProjectExecutionProfile: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
}));

const provider: ProviderView = {
  provider: "codex",
  kind: "codex",
  displayName: "Codex",
  defaultModel: "gpt-6-luna",
  defaultBaseUrl: "https://api.openai.com/v1",
  authModes: ["API_KEY", "CHATGPT_OAUTH"],
  chatgptOauth: "BLOCKED",
  status: "DETERMINISTIC_ONLY",
  note: "Codex App Server route",
  configured: true,
  enabled: true,
  isDefault: false,
  model: "gpt-6-luna",
  baseUrl: "https://api.openai.com/v1",
  credentialFingerprint: "fingerprint",
  revision: 3,
  updatedAt: "2026-10-27T10:00:00Z",
  maxTurns: 4,
  maxInputTokens: 60000,
  maxOutputTokens: 12000,
  maxCostMicros: 500000,
  inputMicrosPerMillion: 1000,
  outputMicrosPerMillion: 2000,
  modelDiscovery: null,
};

function renderSettings(path = "/settings", locale?: Locale) {
  const content = (
    <MemoryRouter initialEntries={[path]}>
      <Settings />
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

describe("Settings", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.lang = "en";
    mocks.listProviders.mockReset().mockResolvedValue({
      providers: [provider],
    });
    mocks.getRuntimeSettings.mockReset().mockResolvedValue({
      semanticReviewEnabled: false,
      semanticReviewMaxOutputTokens: 8192,
      updatedAt: "2026-10-27T10:00:00Z",
    });
    mocks.updateRuntimeSettings.mockReset().mockResolvedValue({
      semanticReviewEnabled: false,
      semanticReviewMaxOutputTokens: 8192,
      updatedAt: "2026-10-27T10:00:00Z",
    });
    mocks.updateProvider.mockReset().mockResolvedValue(provider);
    mocks.testProvider.mockReset().mockResolvedValue({
      provider: "codex",
      status: "AUTHENTICATED",
      models: ["gpt-6-luna"],
      model: "gpt-6-luna",
      modelAvailable: true,
      detail: "model endpoint reachable",
    });
    mocks.listProviderModels.mockReset().mockResolvedValue({
      provider: "codex",
      status: "AVAILABLE",
      models: ["gpt-6-luna", "gpt-6-sol"],
      detail: "2 model(s) discovered",
      discoveredAt: "2026-10-27T10:00:00Z",
      adapter: "codex",
      discovery: "OPENAI_MODELS_ENDPOINT",
    });
    mocks.revokeProviderCredential.mockReset().mockResolvedValue({
      ...provider,
      configured: false,
      enabled: false,
    });
    mocks.providerAudit.mockReset().mockResolvedValue({
      audit: [
        {
          auditId: "audit-1",
          provider: "codex",
          revision: 3,
          action: "UPDATE",
          actor: "owner",
          detail: "route updated",
          createdAt: "2026-10-27T10:00:00Z",
        },
      ],
    });
    mocks.listExecutionProfiles.mockReset().mockResolvedValue({
      profiles: [],
      supportedRoutes: [
        { provider: "opencode-go", model: "deepseek-v4.1-flash" },
      ],
    });
    mocks.listProjects.mockReset().mockResolvedValue([]);
    mocks.getProjectExecutionProfile.mockReset().mockResolvedValue(null);
  });

  it("separates configured route, connection test and proven inference", async () => {
    renderSettings();
    await screen.findByRole("heading", { name: "Settings" });
    expect(screen.getByText("Codex")).toBeDefined();
    expect(screen.getByText("ChatGPT OAuth")).toBeDefined();
    expect(screen.getByText("BLOCKED")).toBeDefined();
    expect(screen.getByText("Configured route")).toBeDefined();
    expect(screen.getByText("Coding inference")).toBeDefined();
    expect(screen.getByText(/No paid inference is authorized/)).toBeDefined();
    expect(screen.getByText("credential configured")).toBeDefined();
    await userEvent.click(screen.getByText("Advanced budget and rate limits"));
    expect(screen.getByLabelText("Max turns")).toBeDefined();
    await userEvent.click(screen.getByText("Configuration audit (1)"));
    expect(screen.getByText("route updated")).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Test connection" }),
    );
    await waitFor(() =>
      expect(screen.getAllByText("AUTHENTICATED").length).toBeGreaterThan(0),
    );
  });

  it("discovers the real provider model catalog and can stage a model", async () => {
    renderSettings();
    await screen.findByRole("heading", { name: "Settings" });
    expect(screen.getByText(/No model discovery is recorded/)).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Discover models" }),
    );
    await waitFor(() =>
      expect(mocks.listProviderModels).toHaveBeenCalledWith("codex"),
    );
    expect(screen.getByText(/2 model\(s\) discovered/)).toBeDefined();
    expect(screen.getByText("gpt-6-sol")).toBeDefined();
    await userEvent.click(screen.getAllByRole("button", { name: "Use" })[1]);
    expect((screen.getByLabelText("Model") as HTMLInputElement).value).toBe(
      "gpt-6-sol",
    );
  });

  it("offers an accessible language switch without leaving the Settings route", async () => {
    renderSettings("/settings?tab=runtime", "en");
    expect(
      await screen.findByRole("heading", { name: "Language" }),
    ).toBeDefined();
    const select = screen.getByLabelText("Interface language");

    for (
      let step = 0;
      step < 4 && document.activeElement !== select;
      step += 1
    ) {
      await userEvent.tab();
    }
    expect(document.activeElement).toBe(select);
    await userEvent.selectOptions(select, "es");

    expect(screen.getByRole("heading", { name: "Idioma" })).toBeDefined();
    expect(screen.getByLabelText("Idioma de la interfaz")).toBeDefined();
    expect(
      screen
        .getByRole("tab", { name: "Runtime policy" })
        .getAttribute("aria-selected"),
    ).toBe("true");
    expect(
      screen.getByRole("heading", { name: "Review and runtime policy" }),
    ).toBeDefined();
    expect(document.documentElement.lang).toBe("es");
    await waitFor(() =>
      expect(localStorage.getItem("factory-office-locale")).toBe("es"),
    );
  });

  it("requires inline confirmation before revoking a credential", async () => {
    renderSettings();
    await screen.findByRole("heading", { name: "Settings" });
    await userEvent.click(
      screen.getByRole("button", { name: "Revoke credential" }),
    );
    expect(
      screen.getByText(/Core will disable this provider route/),
    ).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Confirm revocation" }),
    );
    await waitFor(() =>
      expect(mocks.revokeProviderCredential).toHaveBeenCalledWith("codex"),
    );
  });

  it("saves a replacement credential without echoing it back", async () => {
    renderSettings();
    await screen.findByRole("heading", { name: "Settings" });
    const credential = screen.getByLabelText(
      "Replace credential (leave blank to keep current)",
    );
    await userEvent.type(credential, "secret-provider-key");
    await userEvent.click(
      screen.getByRole("button", { name: "Save configuration" }),
    );
    await waitFor(() =>
      expect(mocks.updateProvider).toHaveBeenCalledWith(
        "codex",
        expect.objectContaining({ credential: "secret-provider-key" }),
      ),
    );
    await waitFor(() =>
      expect(screen.queryByDisplayValue("secret-provider-key")).toBeNull(),
    );
  });

  it("shows an audit read failure as unknown, not empty", async () => {
    mocks.providerAudit.mockRejectedValue(new Error("audit unavailable"));
    renderSettings();
    await screen.findByRole("heading", { name: "Settings" });
    await userEvent.click(screen.getByText("Configuration audit (0)"));
    expect(screen.getByText(/unknown, not empty/)).toBeDefined();
  });

  it("deep-links to execution profiles without rendering provider settings", async () => {
    renderSettings("/settings?tab=profiles");
    await screen.findByRole("heading", {
      name: "Execution profiles (0)",
    });
    expect(
      screen
        .getByRole("tab", { name: "Execution profiles" })
        .getAttribute("aria-selected"),
    ).toBe("true");
    expect(mocks.listExecutionProfiles).toHaveBeenCalled();
    // The profile editor needs the non-secret provider catalog to build its
    // role matrix, but the provider settings cards are not rendered here.
    expect(screen.queryByText("Model catalog")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Discover models" }),
    ).toBeNull();
  });

  it("deep-links to runtime policy and saves it", async () => {
    renderSettings("/settings?tab=runtime");
    await screen.findByRole("heading", {
      name: "Review and runtime policy",
    });
    expect(
      screen
        .getByRole("tab", { name: "Runtime policy" })
        .getAttribute("aria-selected"),
    ).toBe("true");
    await userEvent.click(
      screen.getByLabelText(
        "Enable semantic Reviewer model calls (paid provider route)",
      ),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Save runtime policy" }),
    );
    await waitFor(() =>
      expect(mocks.updateRuntimeSettings).toHaveBeenCalledWith({
        semanticReviewEnabled: true,
        semanticReviewMaxOutputTokens: 8192,
      }),
    );
  });
});
