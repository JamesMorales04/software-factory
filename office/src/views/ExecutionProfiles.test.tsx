import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CoreApiError,
  type ExecutionProfile,
  type ProviderView,
} from "../coreApi";
import { ExecutionProfiles } from "./ExecutionProfiles";

const mocks = vi.hoisted(() => ({
  listExecutionProfiles: vi.fn(),
  getExecutionProfile: vi.fn(),
  createExecutionProfile: vi.fn(),
  updateExecutionProfile: vi.fn(),
  setExecutionProfileDefault: vi.fn(),
  clearExecutionProfileDefault: vi.fn(),
  getProjectExecutionProfile: vi.fn(),
  listProjects: vi.fn(),
  listProviders: vi.fn(),
  listProviderModels: vi.fn(),
}));

vi.mock("../coreApi", () => {
  class MockCoreApiError extends Error {
    readonly status: number;

    constructor(status: number, message: string) {
      super(message);
      this.name = "CoreApiError";
      this.status = status;
    }
  }
  return { coreApi: mocks, CoreApiError: MockCoreApiError };
});

const contentHash = "a".repeat(64);

const supportedRoutes = [
  { provider: "opencode-go", model: "deepseek-v4.1-flash" },
  { provider: "codex", model: "gpt-6-luna" },
  { provider: "opencode", model: "deepseek-v4.1-flash" },
];

const balanced: ExecutionProfile = {
  id: "profile-balanced",
  name: "Balanced",
  description: "Default supported routes",
  category: "BALANCED",
  revision: 3,
  revisionId: "revision-3",
  contentHash,
  globalDefault: true,
  projectDefaults: ["project-1"],
  roles: {
    lead: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
      reasoning: "medium",
      maxInputTokens: 128000,
      maxOutputTokens: 8192,
      maxCostMicros: 300000,
    },
    implementer: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
      reasoning: "medium",
      maxInputTokens: 40000,
      maxOutputTokens: 12000,
      maxCostMicros: 500000,
    },
    semanticReviewer: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
      reasoning: "medium",
      maxInputTokens: 400000,
      maxOutputTokens: 8192,
      maxCostMicros: 1000000,
    },
  },
  budgets: {
    maxAttempts: 3,
    maxSessions: 2,
    maxTurns: 4,
    maxInputTokens: 40000,
    maxOutputTokens: 12000,
    maxCostMicros: 500000,
    deadlineSecs: 600,
  },
  autoStart: { enabled: false, riskClasses: [] },
  executable: true,
  warnings: [],
  createdAt: "2026-10-27T10:00:00Z",
  updatedAt: "2026-10-27T11:00:00Z",
};

const conservative: ExecutionProfile = {
  ...balanced,
  id: "profile-conservative",
  name: "Conservative",
  revision: 1,
  revisionId: "revision-1",
  contentHash: "b".repeat(64),
  globalDefault: false,
  projectDefaults: [],
};

const draft: ExecutionProfile = {
  ...balanced,
  id: "profile-draft",
  name: "Unconfigured draft",
  revision: 1,
  revisionId: "revision-draft",
  globalDefault: false,
  projectDefaults: [],
  roles: {
    ...balanced.roles,
    semanticReviewer: {
      provider: "codex",
      model: "gpt-6-luna",
      reasoning: "medium",
      maxInputTokens: 400000,
      maxOutputTokens: 8192,
      maxCostMicros: 1000000,
    },
  },
  executable: false,
  warnings: [
    "semanticReviewer route codex/gpt-6-luna is declared but codex is not configured",
  ],
};

const providerBase = {
  provider: "opencode-go",
  kind: "opencode",
  displayName: "OpenCode Go",
  defaultModel: "deepseek-v4.1-flash",
  defaultBaseUrl: "https://opencode.ai/inference/go/openai/v1",
  authModes: ["api_key"],
  chatgptOauth: "NOT_APPLICABLE",
  status: "SUPPORTED_TRUSTED_LOCAL",
  note: "test connection",
  configured: true,
  enabled: true,
  isDefault: true,
  model: "deepseek-v4.1-flash",
  baseUrl: "https://provider.example/v1",
  credentialFingerprint: "fingerprint",
  revision: 2,
  updatedAt: "2026-10-27T10:00:00Z",
  maxTurns: 4,
  maxInputTokens: 100000,
  maxOutputTokens: 12000,
  maxCostMicros: 500000,
  inputMicrosPerMillion: 1000,
  outputMicrosPerMillion: 2000,
  modelDiscovery: null,
} satisfies ProviderView;

const providers: ProviderView[] = [
  {
    ...providerBase,
    provider: "opencode-go",
    displayName: "OpenCode Go",
    modelDiscovery: {
      provider: "opencode-go",
      status: "AVAILABLE",
      source: "OPENCODE_GO_CATALOG_URL",
      catalogUrl: "https://opencode.ai/inference/go/v1/models",
      models: ["deepseek-v4.1-flash", "reviewer-model"],
      detail: "2 model(s) discovered",
      discoveredAt: "2026-10-27T10:00:00Z",
      credentialSignal: "NOT_OBSERVED",
      lastKnown: null,
    },
  },
  {
    ...providerBase,
    provider: "opencode",
    displayName: "OpenCode",
    isDefault: false,
    modelDiscovery: null,
  },
  {
    ...providerBase,
    provider: "codex",
    kind: "codex",
    displayName: "Codex App Server",
    status: "DETERMINISTIC_ONLY",
    note: "Codex App Server route",
    configured: false,
    enabled: false,
    isDefault: false,
    model: "gpt-6-luna",
    modelDiscovery: {
      provider: "codex",
      status: "UNSUPPORTED",
      source: "UNSUPPORTED",
      catalogUrl: null,
      models: [],
      detail:
        "the Codex App Server adapter has no OpenAI-compatible model catalog",
      discoveredAt: "2026-10-27T10:00:00Z",
      credentialSignal: "NOT_OBSERVED",
      lastKnown: null,
    },
  },
];

const project = {
  id: "project-1",
  repository: "/repo",
  base_commit: "b".repeat(40),
  created_at: "2026-10-27T10:00:00Z",
};

function mockList(profiles: ExecutionProfile[]) {
  mocks.listExecutionProfiles.mockResolvedValue({ profiles, supportedRoutes });
}

describe("ExecutionProfiles", () => {
  beforeEach(() => {
    mocks.listExecutionProfiles.mockReset();
    mocks.getExecutionProfile.mockReset().mockResolvedValue(balanced);
    mocks.createExecutionProfile
      .mockReset()
      .mockImplementation(
        (input: { name: string; category: ExecutionProfile["category"] }) =>
          Promise.resolve({
            ...balanced,
            id: "profile-new",
            name: input.name,
            category: input.category,
            revision: 1,
            revisionId: "revision-new",
            globalDefault: false,
            projectDefaults: [],
          }),
      );
    mocks.updateExecutionProfile.mockReset().mockResolvedValue({
      ...balanced,
      name: "Renamed",
      revision: 4,
      revisionId: "revision-4",
    });
    mocks.setExecutionProfileDefault.mockReset().mockResolvedValue({
      profileId: balanced.id,
      scope: "global",
      projectId: null,
      revisionId: "revision-3",
      revision: 3,
    });
    mocks.clearExecutionProfileDefault.mockReset().mockResolvedValue(undefined);
    mocks.getProjectExecutionProfile.mockReset().mockResolvedValue({
      projectId: "project-1",
      profileId: "profile-balanced",
      revisionId: "revision-3",
      revision: 3,
      contentHash,
      source: "GLOBAL_DEFAULT",
    });
    mocks.listProjects.mockReset().mockResolvedValue([project]);
    mocks.listProviders.mockReset().mockResolvedValue({ providers });
    mocks.listProviderModels
      .mockReset()
      .mockImplementation((provider: string) => {
        const match = providers.find((entry) => entry.provider === provider);
        if (match?.modelDiscovery) {
          return Promise.resolve({
            ...match.modelDiscovery,
            adapter: match.kind,
            credential: {
              presence: "CONFIGURED",
              validation: "NOT_OBSERVED",
              detail: "test fixture",
            },
            inference: { status: "NOT_TESTED", detail: "test fixture" },
          });
        }
        return Promise.resolve({
          provider,
          status: "UNREACHABLE",
          source: "OPENAI_MODELS_ENDPOINT",
          catalogUrl: null,
          models: [],
          detail: "provider model endpoint is unavailable",
          discoveredAt: "2026-10-27T10:00:00Z",
          credentialSignal: "NOT_OBSERVED",
          lastKnown: null,
          adapter: "opencode",
          credential: {
            presence: "CONFIGURED",
            validation: "NOT_OBSERVED",
            detail: "test fixture",
          },
          inference: { status: "NOT_TESTED", detail: "test fixture" },
        });
      });
    mockList([balanced, conservative]);
  });

  it("shows exact revision, content hash, executability and defaults", async () => {
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (2)",
    });
    expect(screen.getByText(contentHash)).toBeDefined();
    expect(screen.getByText("revision-3")).toBeDefined();
    expect(screen.getAllByText("EXECUTABLE").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("GLOBAL DEFAULT")).toBeDefined();
    expect(screen.getByText("PROJECT project-")).toBeDefined();
    await userEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    await waitFor(() =>
      expect(screen.getByText(/Effective for \/repo/)).toBeDefined(),
    );
  });

  it("creates a profile with contract-shaped snake_case auto_start", async () => {
    mockList([]);
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (0)",
    });
    await userEvent.click(screen.getByRole("button", { name: "New profile" }));
    const name = screen.getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Budget tight");
    await userEvent.selectOptions(screen.getByLabelText("Category"), "BUDGET");
    await userEvent.click(
      screen.getByRole("button", { name: "Create profile" }),
    );
    await waitFor(() =>
      expect(mocks.createExecutionProfile).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "Budget tight",
          description: "Default supported routes",
          category: "BUDGET",
          auto_start: { enabled: false, risk_classes: [] },
          roles: {
            lead: expect.objectContaining({
              provider: "opencode-go",
              model: "deepseek-v4.1-flash",
              reasoning: "medium",
            }),
            implementer: expect.objectContaining({
              provider: "opencode-go",
            }),
            semanticReviewer: expect.objectContaining({
              provider: "opencode-go",
              model: "deepseek-v4.1-flash",
            }),
          },
        }),
      ),
    );
    expect(
      mocks.createExecutionProfile.mock.calls[0]?.[0]?.expected_revision,
    ).toBeUndefined();
    await screen.findByText(/Saved Budget tight revision 1/);
  });

  it("requires a name before calling Core", async () => {
    mockList([]);
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (0)",
    });
    await userEvent.click(screen.getByRole("button", { name: "New profile" }));
    await userEvent.clear(screen.getByLabelText("Name"));
    await userEvent.click(
      screen.getByRole("button", { name: "Create profile" }),
    );
    expect(screen.getByText("Name is required.")).toBeDefined();
    expect(mocks.createExecutionProfile).not.toHaveBeenCalled();
  });

  it("shows a stale revision conflict and can reload the exact revision", async () => {
    mocks.updateExecutionProfile.mockRejectedValue(
      new CoreApiError(409, "execution profile revision is stale"),
    );
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (2)",
    });
    await userEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    const name = screen.getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Renamed");
    await userEvent.click(
      screen.getByRole("button", { name: "Save revision" }),
    );
    expect(screen.getByText(/Core rejected this save/)).toBeDefined();
    expect(screen.getByText(/No revision was written/)).toBeDefined();
    expect(mocks.updateExecutionProfile).toHaveBeenCalledWith(
      "profile-balanced",
      expect.objectContaining({ expected_revision: 3 }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Reload current revision" }),
    );
    await waitFor(() =>
      expect(mocks.getExecutionProfile).toHaveBeenCalledWith(
        "profile-balanced",
      ),
    );
    await waitFor(() =>
      expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe(
        "Balanced",
      ),
    );
    expect(
      (screen.getByLabelText("Semantic Reviewer model") as HTMLSelectElement)
        .value,
    ).toBe("deepseek-v4.1-flash");
  });

  it("sets global and project defaults with explicit scope", async () => {
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (2)",
    });
    await userEvent.click(screen.getAllByRole("button", { name: "Edit" })[1]);
    await userEvent.click(
      screen.getByRole("button", { name: "Set global default" }),
    );
    await waitFor(() =>
      expect(mocks.setExecutionProfileDefault).toHaveBeenCalledWith(
        "profile-conservative",
        { scope: "global" },
      ),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Set project default" }),
    );
    await waitFor(() =>
      expect(mocks.setExecutionProfileDefault).toHaveBeenCalledWith(
        "profile-conservative",
        { scope: "project", project_id: "project-1" },
      ),
    );
    expect(mocks.clearExecutionProfileDefault).not.toHaveBeenCalled();
  });

  it("edits each role provider and selects a discovered model", async () => {
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (2)",
    });
    await userEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    await waitFor(() =>
      expect(mocks.listProviderModels).toHaveBeenCalledWith("opencode-go"),
    );
    const providerSelect = screen.getByLabelText(
      "Semantic Reviewer provider",
    ) as HTMLSelectElement;
    expect(providerSelect.value).toBe("opencode-go");
    expect(
      Array.from(providerSelect.options).map((option) => option.value),
    ).toEqual(expect.arrayContaining(["opencode-go", "opencode", "codex"]));

    await userEvent.type(
      screen.getByLabelText("Search Semantic Reviewer models"),
      "reviewer",
    );
    const modelSelect = screen.getByLabelText(
      "Semantic Reviewer model",
    ) as HTMLSelectElement;
    expect(
      Array.from(modelSelect.options).map((option) => option.value),
    ).toContain("reviewer-model");
    await userEvent.selectOptions(modelSelect, "reviewer-model");
    expect(modelSelect.value).toBe("reviewer-model");

    await userEvent.click(
      screen.getByRole("button", { name: "Save revision" }),
    );
    await waitFor(() =>
      expect(mocks.updateExecutionProfile).toHaveBeenCalledWith(
        "profile-balanced",
        expect.objectContaining({
          roles: expect.objectContaining({
            semanticReviewer: expect.objectContaining({
              provider: "opencode-go",
              model: "reviewer-model",
            }),
          }),
        }),
      ),
    );
  });

  it("shows an unavailable model catalog without inventing model IDs", async () => {
    mockList([draft]);
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (1)",
    });
    await userEvent.click(screen.getByRole("button", { name: "Edit" }));
    await waitFor(() =>
      expect(mocks.listProviderModels).toHaveBeenCalledWith("codex"),
    );
    expect(
      screen.getByText(
        /Discovery unavailable: the Codex App Server adapter has no OpenAI-compatible model catalog/,
      ),
    ).toBeDefined();
    const modelSelect = screen.getByLabelText(
      "Semantic Reviewer model",
    ) as HTMLSelectElement;
    expect(
      Array.from(modelSelect.options).map((option) => option.value),
    ).not.toContain("invented-model");
  });

  it("offers a clearly labelled unverified manual model ID", async () => {
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (2)",
    });
    await userEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    await userEvent.click(
      screen.getByLabelText("Advanced: manual Lead model ID (unverified)"),
    );
    const manual = screen.getByLabelText("Lead manual model ID (unverified)");
    await userEvent.clear(manual);
    await userEvent.type(manual, "manual-model");
    expect(
      screen.getByText(
        /Core keeps a profile non-executable until the provider advertises this exact model/,
      ),
    ).toBeDefined();
    await userEvent.click(
      screen.getByRole("button", { name: "Save revision" }),
    );
    await waitFor(() =>
      expect(mocks.updateExecutionProfile).toHaveBeenCalledWith(
        "profile-balanced",
        expect.objectContaining({
          roles: expect.objectContaining({
            lead: expect.objectContaining({ model: "manual-model" }),
          }),
        }),
      ),
    );
  });

  it("renders a non-executable draft warning and blocks default selection", async () => {
    mockList([draft]);
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (1)",
    });
    await userEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(
      screen.getAllByText(/non-executable draft/).length,
    ).toBeGreaterThanOrEqual(1);
    expect(
      screen.getByText(/semanticReviewer route codex\/gpt-6-luna/),
    ).toBeDefined();
    const globalDefault = screen.getByRole("button", {
      name: "Set global default",
    }) as HTMLButtonElement;
    const projectDefault = screen.getByRole("button", {
      name: "Set project default",
    }) as HTMLButtonElement;
    expect(globalDefault.disabled).toBe(true);
    expect(projectDefault.disabled).toBe(true);
  });
});
