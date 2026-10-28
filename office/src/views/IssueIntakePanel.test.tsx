import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CoreApiError,
  type ExecutionProfile,
  type GitHubIssueSummary,
  type IssueIntakePolicy,
  type IssueIntakeState,
  type ProjectConfig,
  type ProjectGitHubBinding,
} from "../coreApi";
import { IssueIntakePanel } from "./IssueIntakePanel";

const mocks = vi.hoisted(() => ({
  listExecutionProfiles: vi.fn(),
  getProjectConfig: vi.fn(),
  configureIssueIntakePolicy: vi.fn(),
  disableIssueIntakePolicy: vi.fn(),
  createIssueIntakeBatch: vi.fn(),
  revokeIssueIntakeBatch: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  newIdempotencyKey: () => "test-key",
  CoreApiError: class CoreApiError extends Error {
    readonly status: number;
    readonly reason: string | null;
    constructor(status: number, message: string, reason?: string) {
      super(message);
      this.name = "CoreApiError";
      this.status = status;
      this.reason = reason ?? null;
    }
  },
}));

const binding: ProjectGitHubBinding = {
  project_id: "project-1",
  owner: "acme",
  repo: "widgets",
  repo_full_name: "acme/widgets",
  default_branch: "main",
  repository_github_id: 42,
  bound_at: "2026-10-27T11:00:00Z",
  updated_at: "2026-10-27T11:00:00Z",
};

const config: ProjectConfig = {
  projectId: "project-1",
  revision: 1,
  configHash: "c".repeat(64),
  repository: "https://github.com/acme/widgets.git",
  baseCommit: "0123456789abcdef0123456789abcdef01234567",
  allowedPaths: ["src"],
  capabilities: ["repository-edit"],
  qualityProfile: "GENERIC",
  active: true,
  updatedAt: "2026-10-27T10:00:00Z",
};

const profile: ExecutionProfile = {
  id: "profile-1",
  name: "Balanced",
  description: "test",
  category: "BALANCED",
  revision: 2,
  revisionId: "revision-2",
  contentHash: "a".repeat(64),
  globalDefault: false,
  projectDefaults: [],
  roles: {
    lead: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
      maxInputTokens: 1000,
      maxOutputTokens: 1000,
      maxCostMicros: 1000,
    },
    implementer: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
      maxInputTokens: 1000,
      maxOutputTokens: 1000,
      maxCostMicros: 1000,
    },
    semanticReviewer: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
      maxInputTokens: 1000,
      maxOutputTokens: 1000,
      maxCostMicros: 1000,
    },
  },
  budgets: {
    maxAttempts: 2,
    maxSessions: 3,
    maxTurns: 4,
    maxInputTokens: 1000,
    maxOutputTokens: 1000,
    maxCostMicros: 1_000_000,
    deadlineSecs: 1800,
  },
  autoStart: { enabled: true, riskClasses: ["R0"] },
  executable: true,
  warnings: [],
  createdAt: "2026-10-27T10:00:00Z",
  updatedAt: "2026-10-27T10:00:00Z",
};

const policy: IssueIntakePolicy = {
  id: "policy-1",
  project_id: "project-1",
  revision: 1,
  enabled: true,
  repository_full_name: "acme/widgets",
  repository_github_id: 42,
  connection_revision: 2,
  required_labels: ["factory:r0"],
  excluded_labels: ["factory:blocked"],
  author_ids: [7],
  author_associations: ["MEMBER"],
  profile_id: "profile-1",
  profile_revision_id: "revision-2",
  profile_revision: 2,
  profile_hash: "a".repeat(64),
  project_config_hash: "c".repeat(64),
  allowed_paths: ["src"],
  capabilities: ["repository-edit"],
  max_risk_class: "R0",
  max_issues: 3,
  max_concurrent: 1,
  max_cost_micros_per_issue: 300_000,
  max_total_cost_micros: 1_000_000,
  max_attempts: 2,
  max_sessions: 3,
  max_turns: 4,
  max_wall_clock_secs: 900,
  expires_at: "2026-11-27T10:00:00Z",
  acknowledged: true,
  principal_id: "operator-token",
  principal_label: null,
  reason: "enabled deliberately",
  content_hash: "d".repeat(64),
  created_at: "2026-10-27T10:00:00Z",
};

const issue: GitHubIssueSummary = {
  number: 5,
  github_id: 55,
  title: "Fix a bounded bug",
  state: "open",
  state_reason: null,
  labels: [{ name: "factory:r0", color: "00ff00" }],
  author: "octo",
  author_id: 7,
  author_association: "MEMBER",
  html_url: "https://github.com/acme/widgets/issues/5",
  comments: 0,
  created_at: "2026-10-26T10:00:00Z",
  updated_at: "2026-10-27T09:00:00Z",
  closed_at: null,
  synced_at: "2026-10-27T12:00:00Z",
  content_hash: "e".repeat(64),
};

function state(overrides: Partial<IssueIntakeState> = {}): IssueIntakeState {
  return {
    project_id: "project-1",
    enabled: true,
    policy,
    policy_history: [policy],
    batches: [],
    queue: [],
    audit: [],
    binding,
    connection_enabled: true,
    connection_revision: 2,
    limits: {
      max_batch_issues: 20,
      max_concurrent: 1,
      max_queue_attempts: 3,
      queue_lease_seconds: 45,
      autonomous_start: false,
      campaign_start: false,
      provider_calls: false,
    },
    ...overrides,
  };
}

function renderPanel(
  overrides: {
    state?: IssueIntakeState | null;
    selected?: GitHubIssueSummary[];
    onRefresh?: () => void;
    onSelectionClear?: () => void;
    loadError?: string | null;
  } = {},
) {
  return render(
    <IssueIntakePanel
      projectId="project-1"
      binding={binding}
      connectionConfigured
      state={overrides.state === undefined ? state() : overrides.state}
      loading={false}
      loadError={overrides.loadError ?? null}
      selected={overrides.selected ?? []}
      onRefresh={overrides.onRefresh ?? (() => {})}
      onSelectionClear={overrides.onSelectionClear ?? (() => {})}
    />,
  );
}

beforeEach(() => {
  mocks.listExecutionProfiles.mockReset().mockResolvedValue({
    profiles: [profile],
    supportedRoutes: [],
  });
  mocks.getProjectConfig.mockReset().mockResolvedValue(config);
  mocks.configureIssueIntakePolicy.mockReset();
  mocks.disableIssueIntakePolicy.mockReset();
  mocks.createIssueIntakeBatch.mockReset();
  mocks.revokeIssueIntakeBatch.mockReset();
});

describe("IssueIntakePanel", () => {
  it("shows issue intake as default OFF and requires acknowledgement", async () => {
    renderPanel({ state: state({ enabled: false, policy: null }) });
    expect(await screen.findByText("OFF")).toBeTruthy();
    const enable = screen.getByRole("button", {
      name: "Enable issue intake",
    }) as HTMLButtonElement;
    expect(enable.disabled).toBe(true);
    expect(screen.getByText(/Default off\./)).toBeTruthy();
  });

  it("renders enabled policy state without claiming execution authority", () => {
    renderPanel();
    expect(screen.getByText("ENABLED")).toBeTruthy();
    expect(screen.getByText(/Neither starts a Campaign/)).toBeTruthy();
    expect(screen.getByText(/not granted by this policy/)).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Acknowledge finite batch" }),
    ).toBeTruthy();
  });

  it("acknowledges exact selected snapshots and clears only after success", async () => {
    const onRefresh = vi.fn();
    const onSelectionClear = vi.fn();
    mocks.createIssueIntakeBatch.mockResolvedValue({
      batch: { id: "batch-1" },
      items: [],
      created: true,
    });
    renderPanel({ selected: [issue], onRefresh, onSelectionClear });
    await userEvent.type(
      screen.getByLabelText("Batch reason"),
      "selected exact snapshot",
    );
    await userEvent.click(
      screen.getByLabelText(
        /I acknowledge these exact issue IDs and content digests/,
      ),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Acknowledge finite batch" }),
    );
    await waitFor(() =>
      expect(mocks.createIssueIntakeBatch).toHaveBeenCalled(),
    );
    const [, request] = mocks.createIssueIntakeBatch.mock.calls[0];
    expect(request.items).toEqual([
      {
        number: 5,
        github_issue_id: 55,
        content_hash: issue.content_hash,
        updated_at: issue.updated_at,
      },
    ]);
    expect(request.acknowledge).toBe(true);
    expect(onSelectionClear).toHaveBeenCalled();
    expect(onRefresh).toHaveBeenCalled();
  });

  it("shows stale-snapshot conflicts and current eligibility reasons", async () => {
    const onRefresh = vi.fn();
    mocks.createIssueIntakeBatch.mockRejectedValue(
      new CoreApiError(
        409,
        "a selected issue changed after the operator viewed it",
        "ISSUE_SNAPSHOT_STALE",
      ),
    );
    renderPanel({
      state: state({
        batches: [
          {
            batch: {
              id: "batch-1",
              project_id: "project-1",
              policy_id: "policy-1",
              policy_revision: 1,
              policy_hash: "d".repeat(64),
              delegation_id: "delegation-1",
              repository_full_name: "acme/widgets",
              repository_github_id: 42,
              connection_revision: 2,
              profile_id: "profile-1",
              profile_revision_id: "revision-2",
              profile_revision: 2,
              profile_hash: "a".repeat(64),
              project_config_hash: "c".repeat(64),
              max_issues: 1,
              max_concurrent: 1,
              max_cost_micros_per_issue: 300_000,
              max_total_cost_micros: 300_000,
              reserved_cost_micros: 0,
              reserved_issues: 0,
              status: "ACTIVE",
              acknowledged: true,
              principal_id: "operator-token",
              principal_label: null,
              reason: "batch",
              idempotency_key: "key",
              batch_hash: "f".repeat(64),
              expires_at: "2026-11-27T10:00:00Z",
              created_at: "2026-10-27T10:00:00Z",
              updated_at: "2026-10-27T10:00:00Z",
              revoked_at: null,
            },
            items: [
              {
                item: {
                  id: "item-1",
                  batch_id: "batch-1",
                  project_id: "project-1",
                  repository_full_name: "acme/widgets",
                  repository_github_id: 42,
                  issue_number: 5,
                  github_issue_id: 55,
                  content_hash: issue.content_hash,
                  issue_updated_at: issue.updated_at,
                  author_login: "octo",
                  author_id: 7,
                  author_association: "MEMBER",
                  labels: ["factory:r0"],
                  state: "PENDING",
                  eligibility_reasons: [],
                  reserved_cost_micros: 0,
                  intake_request_id: null,
                  queue_item_id: "queue-1",
                  created_at: "2026-10-27T10:00:00Z",
                  updated_at: "2026-10-27T10:00:00Z",
                },
                current_eligibility: {
                  eligible: false,
                  reasons: ["CONNECTION_REVISION_CHANGED"],
                },
              },
            ],
            remaining_issues: 1,
            remaining_cost_micros: 300_000,
          },
        ],
      }),
      selected: [issue],
      onRefresh,
    });
    expect(screen.getByText("CONNECTION_REVISION_CHANGED")).toBeTruthy();
    await userEvent.type(
      screen.getByLabelText("Batch reason"),
      "retry exact snapshot",
    );
    await userEvent.click(
      screen.getByLabelText(
        /I acknowledge these exact issue IDs and content digests/,
      ),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Acknowledge finite batch" }),
    );
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(
      screen.getByText(/a selected issue changed after the operator viewed it/),
    ).toBeTruthy();
    expect(onRefresh).toHaveBeenCalled();
  });

  it("requires a confirmation reason before disabling", async () => {
    mocks.disableIssueIntakePolicy.mockResolvedValue({
      policy: { ...policy, enabled: false, revision: 2 },
      created: true,
    });
    renderPanel();
    await userEvent.click(
      screen.getByRole("button", { name: "Disable issue intake" }),
    );
    await userEvent.type(
      screen.getByLabelText("Disable reason"),
      "owner disabled intake",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Confirm disable" }),
    );
    await waitFor(() =>
      expect(mocks.disableIssueIntakePolicy).toHaveBeenCalledWith(
        "project-1",
        expect.objectContaining({ reason: "owner disabled intake" }),
      ),
    );
  });
});
