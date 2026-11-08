import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  GitHubConnectionState,
  GitHubIssueList,
  IssueIntakeState,
  Project,
  ProjectConfig,
} from "../coreApi";
import { I18nProvider } from "../i18n";
import { Issues } from "./Issues";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  getGitHubConnection: vi.fn(),
  getProjectGitHub: vi.fn(),
  listProjectIssues: vi.fn(),
  getIssueIntake: vi.fn(),
  syncProjectIssues: vi.fn(),
  updateGitHubConnection: vi.fn(),
  listGitHubRepositories: vi.fn(),
  bindProjectGitHub: vi.fn(),
  getProjectConfig: vi.fn(),
  getProjectExecutionProfile: vi.fn(),
  configureIssueIntakePolicy: vi.fn(),
  createIssueIntakeBatch: vi.fn(),
  dispatchIssueIntake: vi.fn(),
  listIntakeRequests: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
  newIdempotencyKey: () => "idempotency-key",
}));

const project: Project = {
  id: "project-1",
  name: "Widgets",
  repository: "https://github.com/acme/widgets.git",
  base_commit: "a".repeat(40),
  created_at: "2026-11-08T00:00:00Z",
  source_kind: "github",
};

const connection: GitHubConnectionState = {
  connection: {
    configured: true,
    enabled: true,
    login: "acme",
    scopes: [],
    scopes_reported: false,
    revision: 1,
    verified_at: "2026-11-08T00:00:00Z",
    revoked_at: null,
    detail: "",
    credential_fingerprint: "fingerprint",
    updated_at: "2026-11-08T00:00:00Z",
  },
  audit: [],
};

const binding = {
  project_id: project.id,
  owner: "acme",
  repo: "widgets",
  repo_full_name: "acme/widgets",
  default_branch: "main",
  repository_github_id: 42,
  bound_at: "2026-11-08T00:00:00Z",
  updated_at: "2026-11-08T00:00:00Z",
};

const issue = {
  number: 7,
  github_id: 700,
  title: "Fix the widget",
  state: "open" as const,
  state_reason: null,
  labels: [{ name: "factory:r0", color: "00ff00" }],
  author: "octo",
  author_id: 1,
  author_association: "MEMBER",
  html_url: "https://github.com/acme/widgets/issues/7",
  comments: 0,
  created_at: "2026-11-08T00:00:00Z",
  updated_at: "2026-11-08T00:00:00Z",
  closed_at: null,
  synced_at: "2026-11-08T00:00:00Z",
  content_hash: "hash-7",
};

const issueList: GitHubIssueList = {
  items: [issue],
  total: 1,
  limit: 100,
  offset: 0,
  synced_at: "2026-11-08T00:00:00Z",
  stale: false,
  sync_status: "OK",
  last_error: null,
  next_allowed_at: null,
  truncated: false,
  binding,
};

const policy = {
  id: "policy-1",
  project_id: project.id,
  revision: 1,
  enabled: true,
  repository_full_name: "acme/widgets",
  repository_github_id: 42,
  connection_revision: 1,
  required_labels: ["factory:r0"],
  excluded_labels: [],
  author_ids: [],
  author_associations: ["MEMBER"],
  profile_id: "profile-1",
  profile_revision_id: "revision-1",
  profile_revision: 1,
  profile_hash: "b".repeat(64),
  project_config_hash: "config-hash",
  allowed_paths: ["src"],
  capabilities: ["repository-edit"],
  max_risk_class: "R0" as const,
  max_issues: 3,
  max_concurrent: 1,
  max_cost_micros_per_issue: 1,
  max_total_cost_micros: 1,
  max_attempts: 1,
  max_sessions: 1,
  max_turns: 1,
  max_wall_clock_secs: 1,
  expires_at: "2026-11-15T00:00:00Z",
  acknowledged: true,
  principal_id: "owner",
  principal_label: "Owner",
  reason: "test policy",
  content_hash: "policy-hash",
  created_at: "2026-11-08T00:00:00Z",
};

const intake: IssueIntakeState = {
  project_id: project.id,
  enabled: true,
  policy,
  policy_history: [],
  batches: [],
  queue: [],
  audit: [],
  binding,
  connection_enabled: true,
  connection_revision: 1,
  limits: {
    max_batch_issues: 20,
    max_concurrent: 1,
    max_queue_attempts: 3,
    queue_lease_seconds: 45,
    autonomous_start: false,
    campaign_start: false,
    provider_calls: false,
  },
};

const config: ProjectConfig = {
  projectId: project.id,
  revision: 1,
  configHash: "config-hash",
  repository: project.repository,
  baseCommit: project.base_commit,
  allowedPaths: ["src"],
  capabilities: ["repository-edit"],
  qualityProfile: "generic",
  active: true,
  updatedAt: "2026-11-08T00:00:00Z",
};

function renderIssues() {
  return render(
    <MemoryRouter>
      <I18nProvider initialLocale="en">
        <Issues />
      </I18nProvider>
    </MemoryRouter>,
  );
}

describe("Issues board", () => {
  beforeEach(() => {
    localStorage.clear();
    Object.values(mocks).forEach((mock) => {
      mock.mockReset();
    });
    mocks.listProjects.mockResolvedValue([project]);
    mocks.listIntakeRequests.mockResolvedValue([]);
    mocks.getGitHubConnection.mockResolvedValue(connection);
    mocks.getProjectGitHub.mockResolvedValue({
      binding,
      sync_state: { status: "OK", stale: false },
    });
    mocks.listProjectIssues.mockResolvedValue(issueList);
    mocks.getIssueIntake.mockResolvedValue(intake);
    mocks.getProjectConfig.mockResolvedValue(config);
    mocks.getProjectExecutionProfile.mockResolvedValue({
      projectId: project.id,
      profileId: "profile-1",
      revisionId: "revision-1",
      revision: 1,
      contentHash: "b".repeat(64),
      source: "GLOBAL_DEFAULT",
    });
    mocks.configureIssueIntakePolicy.mockResolvedValue({
      policy,
      created: true,
    });
    mocks.createIssueIntakeBatch.mockResolvedValue({
      batch: {
        id: "batch-1",
        project_id: project.id,
        policy_id: policy.id,
        policy_revision: 1,
        policy_hash: policy.content_hash,
        delegation_id: "delegation-1",
        repository_full_name: "acme/widgets",
        repository_github_id: 42,
        connection_revision: 1,
        profile_id: policy.profile_id,
        profile_revision_id: policy.profile_revision_id,
        profile_revision: 1,
        profile_hash: policy.profile_hash,
        project_config_hash: policy.project_config_hash,
        status: "ACTIVE",
        max_issues: 3,
        reserved_issues: 0,
        max_cost_micros_per_issue: 1,
        max_total_cost_micros: 1,
        reserved_cost_micros: 0,
        expires_at: "2026-11-09T00:00:00Z",
        acknowledged: true,
        principal_id: "owner",
        reason: "test",
        created_at: "2026-11-08T00:00:00Z",
        updated_at: "2026-11-08T00:00:00Z",
        revoked_at: null,
      },
      items: [],
      created: true,
    });
    mocks.dispatchIssueIntake.mockResolvedValue({
      status: "RECONCILED",
      reasons: [],
      queue: null,
      intake: {
        id: "request-1",
        lead_request_id: "lead-1",
        action_id: "action-1",
      },
    });
    mocks.syncProjectIssues.mockResolvedValue({ status: "OK" });
  });

  it("renders issues by real state and auto-syncs a stale project", async () => {
    mocks.getProjectGitHub.mockResolvedValue({
      binding,
      sync_state: { status: "NEVER_SYNCED", stale: true },
    });
    renderIssues();
    expect(await screen.findByText(/Fix the widget/)).toBeDefined();
    expect(screen.getByRole("heading", { name: "Ready" })).toBeDefined();
    await waitFor(() =>
      expect(mocks.syncProjectIssues).toHaveBeenCalledWith(project.id),
    );
  });

  it("shows closed issues in the Done column", async () => {
    mocks.listProjectIssues.mockResolvedValue({
      ...issueList,
      items: [{ ...issue, state: "closed" }],
      total: 1,
    });
    renderIssues();
    expect(await screen.findByText(/Fix the widget/)).toBeDefined();
    const done = screen
      .getByRole("heading", { name: "Done" })
      .closest("section");
    expect(done?.textContent).toContain("Fix the widget");
  });

  it("distinguishes a failed Project read from an empty board", async () => {
    mocks.listProjectIssues.mockRejectedValue(new Error("Core unavailable"));
    renderIssues();
    expect(
      await screen.findByText("Some Projects could not be loaded"),
    ).toBeDefined();
    expect(screen.getAllByText("Widgets").length).toBeGreaterThan(0);
  });

  it("refreshes a bound repository on demand", async () => {
    const user = userEvent.setup();
    renderIssues();
    await screen.findByText(/Fix the widget/);
    await waitFor(() =>
      expect(
        screen
          .getByRole("button", { name: "Refresh issues" })
          .hasAttribute("disabled"),
      ).toBe(false),
    );
    await user.click(screen.getByRole("button", { name: "Refresh issues" }));
    await waitFor(() =>
      expect(mocks.syncProjectIssues).toHaveBeenCalledWith(project.id),
    );
  });

  it("starts one governed work request from a ready issue", async () => {
    const user = userEvent.setup();
    renderIssues();
    await user.click(
      await screen.findByRole("button", { name: /Fix the widget/ }),
    );
    await user.click(screen.getByRole("button", { name: "Start work" }));
    await waitFor(() =>
      expect(mocks.createIssueIntakeBatch).toHaveBeenCalledWith(
        project.id,
        expect.objectContaining({ policy_id: "policy-1" }),
      ),
    );
    expect(mocks.dispatchIssueIntake).toHaveBeenCalledWith(
      project.id,
      "batch-1",
    );
  });
});
