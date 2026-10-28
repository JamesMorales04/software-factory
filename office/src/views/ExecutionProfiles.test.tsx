import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CoreApiError, type ExecutionProfile } from "../coreApi";
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
      provider: "codex",
      model: "gpt-6-luna",
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
  executable: false,
  warnings: [
    "semanticReviewer route codex/gpt-6-luna is declared but codex is not configured",
  ],
};

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
              provider: "codex",
              model: "gpt-6-luna",
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

  it("renders a non-executable draft warning and blocks default selection", async () => {
    mockList([draft]);
    render(<ExecutionProfiles />);
    await screen.findByRole("heading", {
      name: "Execution profiles (1)",
    });
    await userEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByText(/non-executable draft/)).toBeDefined();
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
