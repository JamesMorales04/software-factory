import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ExecutionProfile, Project } from "../coreApi";
import { writeWorkspaceSelection } from "../workspace";
import { WorkspaceContextBar } from "./WorkspaceContextBar";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  listExecutionProfiles: vi.fn(),
}));

vi.mock("../coreApi", () => ({ coreApi: mocks }));

const projects = [
  {
    id: "project-1",
    repository: "acme/widgets",
    name: "Widgets",
    base_commit: "a".repeat(40),
    created_at: "2026-11-06T00:00:00Z",
  },
  {
    id: "project-2",
    repository: "acme/telemetry",
    name: "Telemetry",
    base_commit: "b".repeat(40),
    created_at: "2026-11-06T00:00:00Z",
  },
] as Project[];

const profiles = [
  {
    id: "profile-1",
    name: "Balanced",
    executable: true,
    globalDefault: true,
    projectDefaults: [],
    revision: 1,
    revisionId: "revision-1",
    contentHash: "c".repeat(64),
    roles: {
      lead: { provider: "opencode-go", model: "deepseek" },
      implementer: { provider: "opencode-go", model: "deepseek" },
      semanticReviewer: { provider: "codex", model: "gpt-6-luna" },
    },
    operations: {},
    progress: {},
    autoStart: { enabled: false, riskClasses: [] },
    warnings: [],
  },
] as unknown as ExecutionProfile[];

describe("WorkspaceContextBar", () => {
  beforeEach(() => {
    localStorage.clear();
    mocks.listProjects.mockReset().mockResolvedValue(projects);
    mocks.listExecutionProfiles.mockReset().mockResolvedValue({ profiles });
  });

  it("uses Core defaults, persists changes and follows workspace updates", async () => {
    render(<WorkspaceContextBar />);

    const projectSelect = (await screen.findByLabelText(
      "Workspace project",
    )) as HTMLSelectElement;
    await waitFor(() => expect(projectSelect.value).toBe("project-1"));
    expect(
      (screen.getByLabelText("Workspace quality profile") as HTMLSelectElement)
        .value,
    ).toBe("profile-1");

    fireEvent.change(projectSelect, { target: { value: "project-2" } });
    await waitFor(() => expect(projectSelect.value).toBe("project-2"));
    expect(localStorage.getItem("factory-office-project")).toBe("project-2");

    writeWorkspaceSelection({ projectId: "project-1" });
    await waitFor(() => expect(projectSelect.value).toBe("project-1"));
  });

  it("explains an unavailable Core catalog without inventing options", async () => {
    mocks.listProjects.mockReset().mockRejectedValue(new Error("offline"));
    mocks.listExecutionProfiles
      .mockReset()
      .mockRejectedValue(new Error("offline"));

    render(<WorkspaceContextBar />);

    await waitFor(() =>
      expect(
        (screen.getByLabelText("Workspace project") as HTMLSelectElement)
          .disabled,
      ).toBe(true),
    );
    expect(screen.getAllByText("Unavailable").length).toBeGreaterThan(0);
  });
});
