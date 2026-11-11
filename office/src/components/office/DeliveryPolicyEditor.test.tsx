import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectDeliveryPolicyView } from "../../coreApi";
import { I18nProvider } from "../../i18n";
import { DeliveryPolicyEditor } from "./DeliveryPolicyEditor";

const mocks = vi.hoisted(() => ({
  getProjectDeliveryPolicy: vi.fn(),
  updateProjectDeliveryPolicy: vi.fn(),
}));

vi.mock("../../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {
    status: number;
    body: unknown;
    constructor(status: number, message: string, body?: unknown) {
      super(message);
      this.status = status;
      this.body = body ?? null;
    }
  },
}));

const view: ProjectDeliveryPolicyView = {
  policy: null,
  revisions: [],
  binding: {
    project_id: "project-1",
    owner: "acme",
    repo: "widgets",
    repo_full_name: "acme/widgets",
    default_branch: "main",
    repository_github_id: null,
    bound_at: "2026-11-01T00:00:00Z",
    updated_at: "2026-11-01T00:00:00Z",
  },
  default_branch: "main",
  merge_method: "squash",
  source: "test",
};

function renderEditor() {
  render(
    <I18nProvider>
      <DeliveryPolicyEditor projectId="project-1" />
    </I18nProvider>,
  );
}

describe("DeliveryPolicyEditor", () => {
  beforeEach(() => {
    mocks.getProjectDeliveryPolicy.mockReset();
    mocks.updateProjectDeliveryPolicy.mockReset();
  });

  it("appends a policy revision through an authenticated Core operation", async () => {
    mocks.getProjectDeliveryPolicy.mockResolvedValue(view);
    mocks.updateProjectDeliveryPolicy.mockResolvedValue({
      policy: {
        schemaVersion: 1,
        projectId: "project-1",
        revision: 1,
        policySha256: "a".repeat(64),
        mode: "RISK_TIERED",
        r1Enabled: true,
        autoMergeBaseBranch: "main",
        manualBaseBranches: ["main"],
        requiredChecks: ["ci", "test"],
        mergeMethod: "squash",
        postMergeCheckRequired: true,
        postMergeCheckId: "post-merge",
        postMergeTimeoutSecs: 600,
        killSwitch: false,
        createdBy: "office-operator",
        createdAt: "2026-11-02T00:00:00Z",
        expiresAt: null,
      },
      created: true,
      binding: view.binding,
    });

    const user = userEvent.setup();
    renderEditor();
    expect(
      await screen.findByRole("heading", { name: "Delivery policy" }),
    ).toBeDefined();
    expect(screen.getByText(/No policy revision exists yet/)).toBeDefined();

    await user.click(
      screen.getByRole("checkbox", {
        name: "Enable risk-tiered delivery",
      }),
    );
    await user.click(
      screen.getByRole("checkbox", {
        name: "Allow R1 auto-merge with a matching Owner Charter grant",
      }),
    );
    await user.clear(screen.getByLabelText("Required checks"));
    await user.type(screen.getByLabelText("Required checks"), "ci, test");
    await user.clear(screen.getByLabelText("Post-merge check"));
    await user.type(screen.getByLabelText("Post-merge check"), "post-merge");
    await user.clear(screen.getByLabelText("Post-merge timeout (seconds)"));
    await user.type(
      screen.getByLabelText("Post-merge timeout (seconds)"),
      "600",
    );
    await user.click(
      screen.getByRole("button", { name: "Append policy revision" }),
    );

    await waitFor(() =>
      expect(mocks.updateProjectDeliveryPolicy).toHaveBeenCalledWith(
        "project-1",
        {
          enabled: true,
          r1_enabled: true,
          merge_method: "squash",
          required_checks: ["ci", "test"],
          post_merge_check_id: "post-merge",
          post_merge_timeout_secs: 600,
          kill_switch: false,
          expires_at: null,
          manual_base_branches: ["main"],
        },
      ),
    );
    expect(
      await screen.findByText("Delivery policy revision appended."),
    ).toBeDefined();
    expect(screen.getByText(/Current revision 1/)).toBeDefined();
  });

  it("displays and submits non-default manual base branches", async () => {
    const policy = {
      schemaVersion: 1,
      projectId: "project-1",
      revision: 1,
      policySha256: "b".repeat(64),
      mode: "MANUAL" as const,
      r1Enabled: false,
      autoMergeBaseBranch: "main",
      manualBaseBranches: ["main", "release"],
      requiredChecks: ["ci"],
      mergeMethod: "squash",
      postMergeCheckRequired: true,
      postMergeCheckId: "post-merge",
      postMergeTimeoutSecs: 900,
      killSwitch: false,
      createdBy: "owner",
      createdAt: "2026-11-02T00:00:00Z",
      expiresAt: null,
    };
    mocks.getProjectDeliveryPolicy.mockResolvedValue({
      ...view,
      policy,
      revisions: [policy],
    });
    mocks.updateProjectDeliveryPolicy.mockResolvedValue({
      policy: {
        ...policy,
        revision: 2,
        manualBaseBranches: ["main", "release", "hotfix"],
      },
      created: true,
      binding: view.binding,
    });
    const user = userEvent.setup();
    renderEditor();
    const input = (await screen.findByLabelText(
      "Manual base branches",
    )) as HTMLInputElement;
    expect(input.value).toBe("main, release");
    await user.clear(input);
    await user.type(input, "main, release, hotfix");
    await user.click(
      screen.getByRole("button", { name: "Append policy revision" }),
    );
    await waitFor(() =>
      expect(mocks.updateProjectDeliveryPolicy).toHaveBeenCalledWith(
        "project-1",
        expect.objectContaining({
          manual_base_branches: ["main", "release", "hotfix"],
        }),
      ),
    );
  });

  it("shows the unbound state instead of a decorative editor", async () => {
    mocks.getProjectDeliveryPolicy.mockResolvedValue({
      ...view,
      binding: null,
      default_branch: null,
    });
    renderEditor();
    expect(
      await screen.findByText(/Bind this Project to a GitHub repository/),
    ).toBeDefined();
    expect(
      screen.queryByRole("button", { name: "Append policy revision" }),
    ).toBeNull();
  });

  it("renders an error with retry when Core is unavailable", async () => {
    mocks.getProjectDeliveryPolicy
      .mockRejectedValueOnce(new Error("core unavailable"))
      .mockResolvedValueOnce(view);
    const user = userEvent.setup();
    renderEditor();
    expect(
      await screen.findByText(
        "Core could not read the Project delivery policy.",
      ),
    ).toBeDefined();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("heading", { name: "Delivery policy" }),
    ).toBeDefined();
  });
});
