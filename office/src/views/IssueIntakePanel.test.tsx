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
import { I18nProvider } from "../i18n";
import { IssueIntakePanel } from "./IssueIntakePanel";

const mocks = vi.hoisted(() => ({
  listExecutionProfiles: vi.fn(),
  getProjectConfig: vi.fn(),
  configureIssueIntakePolicy: vi.fn(),
  disableIssueIntakePolicy: vi.fn(),
  createIssueIntakeBatch: vi.fn(),
  revokeIssueIntakeBatch: vi.fn(),
  dispatchIssueIntake: vi.fn(),
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
  schemaVersion: 2,
  revision: 2,
  revisionId: "revision-2",
  contentHash: "a".repeat(64),
  globalDefault: false,
  projectDefaults: [],
  roles: {
    lead: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
    },
    implementer: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
    },
    semanticReviewer: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
    },
  },
  operations: {
    modelRequestTimeoutSecs: 300,
    toolCallTimeoutSecs: 600,
    maxRequestBytes: 2097152,
    maxResponseBytes: 2097152,
    maxGenerationTokens: 8192,
    maxInFlightOperations: 4,
  },
  progress: {
    diagnoseAfterStagnantCheckpoints: 2,
    escalateAfterUnsuccessfulReplans: 1,
    observationTimeoutSecs: 300,
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

function activeBatchState(): IssueIntakeState["batches"][number] {
  return {
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
      status: "ACTIVE" as const,
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
          state: "PENDING" as const,
          eligibility_reasons: [],
          reserved_cost_micros: 0,
          intake_request_id: null,
          queue_item_id: "queue-1",
          created_at: "2026-10-27T10:00:00Z",
          updated_at: "2026-10-27T10:00:00Z",
        },
        current_eligibility: { eligible: true, reasons: [] },
      },
    ],
    remaining_issues: 1,
    remaining_cost_micros: 300_000,
  };
}

function queuedItem(
  overrides: Partial<IssueIntakeState["queue"][number]> = {},
): IssueIntakeState["queue"][number] {
  return {
    id: "queue-1",
    project_id: "project-1",
    batch_id: "batch-1",
    batch_item_id: "item-1",
    repository_full_name: "acme/widgets",
    repository_github_id: 42,
    issue_number: 5,
    github_issue_id: 55,
    content_hash: issue.content_hash,
    connection_revision: 2,
    policy_revision: 1,
    status: "PENDING",
    lease_token: null,
    lease_expires_at: null,
    attempts: 0,
    max_attempts: 3,
    next_attempt_at: null,
    reason: null,
    reasons: [],
    intake_request_id: null,
    lead_request_id: null,
    action_id: null,
    created_at: "2026-10-27T10:00:00Z",
    updated_at: "2026-10-27T10:00:00Z",
    completed_at: null,
    ...overrides,
  };
}

function renderPanel(
  overrides: {
    locale?: "en" | "es";
    state?: IssueIntakeState | null;
    selected?: GitHubIssueSummary[];
    onRefresh?: () => void;
    onSelectionClear?: () => void;
    loadError?: string | null;
  } = {},
) {
  return render(
    <I18nProvider initialLocale={overrides.locale ?? "en"}>
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
      />
    </I18nProvider>,
  );
}

beforeEach(() => {
  localStorage.clear();
  mocks.listExecutionProfiles.mockReset().mockResolvedValue({
    profiles: [profile],
    supportedRoutes: [],
  });
  mocks.getProjectConfig.mockReset().mockResolvedValue(config);
  mocks.configureIssueIntakePolicy.mockReset();
  mocks.disableIssueIntakePolicy.mockReset();
  mocks.createIssueIntakeBatch.mockReset();
  mocks.revokeIssueIntakeBatch.mockReset();
  mocks.dispatchIssueIntake.mockReset();
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

  it("dispatches one exact queue item and reports that no Campaign started", async () => {
    const batchState = activeBatchState();
    const queued = queuedItem();
    mocks.dispatchIssueIntake.mockResolvedValue({
      status: "RECONCILED",
      reasons: [],
      queue: {
        ...queued,
        status: "RECONCILED",
        intake_request_id: "intake-1",
        lead_request_id: "lead-1",
      },
      intake: {
        id: "intake-1",
        lead_request_id: "lead-1",
        action_id: "action-1",
        objective: "Resolve issue",
        mode: "REVIEW_PLAN",
      },
      campaign_started: false,
      provider_calls: 0,
    });
    const onRefresh = vi.fn();
    renderPanel({
      state: state({ batches: [batchState], queue: [queued] }),
      onRefresh,
    });
    await userEvent.click(
      screen.getByRole("button", { name: "Create proposal" }),
    );
    await waitFor(() =>
      expect(mocks.dispatchIssueIntake).toHaveBeenCalledWith(
        "project-1",
        "batch-1",
      ),
    );
    expect(await screen.findByText(/No Campaign was started/)).toBeTruthy();
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

  it("revokes a batch with a Spanish default audit reason", async () => {
    mocks.revokeIssueIntakeBatch.mockResolvedValue({
      batch: { ...activeBatchState().batch, status: "REVOKED" },
    });
    renderPanel({
      locale: "es",
      state: state({ batches: [activeBatchState()] }),
    });
    await userEvent.click(screen.getByRole("button", { name: "Revocar lote" }));
    await waitFor(() =>
      expect(mocks.revokeIssueIntakeBatch).toHaveBeenCalledWith(
        "project-1",
        "batch-1",
        { reason: "el operador revocó el lote de incidencias" },
      ),
    );
    expect(
      await screen.findByText(
        "Lote revocado. Se conservan las reservas existentes.",
      ),
    ).toBeTruthy();
  });

  it("renders the governed issue-intake policy form in Spanish", async () => {
    renderPanel({
      locale: "es",
      state: state({ enabled: false, policy: null }),
    });
    expect(await screen.findByText("DESACTIVADA")).toBeTruthy();
    expect(
      screen.getByRole("heading", {
        name: "Admisión de incidencias gobernada",
      }),
    ).toBeTruthy();
    expect(screen.getByText(/Desactivada por defecto/)).toBeTruthy();
    expect(screen.getByLabelText("Perfil de ejecución")).toBeTruthy();
    expect(
      screen.getByLabelText("Etiquetas obligatorias (separadas por comas)"),
    ).toBeTruthy();
    expect(screen.getByLabelText("Motivo del reconocimiento")).toBeTruthy();
    expect(
      screen.getByLabelText(/Reconozco esta política exacta/),
    ).toBeTruthy();
    expect(
      await screen.findByText(/Este perfil tiene el inicio automático/),
    ).toBeTruthy();
    const enable = screen.getByRole("button", {
      name: "Habilitar la admisión de incidencias",
    }) as HTMLButtonElement;
    expect(enable.disabled).toBe(true);
  });

  it("enables the policy from the Spanish form and reports the revision", async () => {
    const onRefresh = vi.fn();
    mocks.configureIssueIntakePolicy.mockResolvedValue({
      policy: { ...policy, revision: 2 },
      created: false,
    });
    renderPanel({
      locale: "es",
      state: state({ enabled: false, policy: null }),
      onRefresh,
    });
    await screen.findByText(/Este perfil tiene el inicio automático/);
    await userEvent.type(
      screen.getByLabelText("Motivo del reconocimiento"),
      "política R0 deliberada y acotada",
    );
    await userEvent.click(
      screen.getByLabelText(/Reconozco esta política exacta/),
    );
    const enable = screen.getByRole("button", {
      name: "Habilitar la admisión de incidencias",
    }) as HTMLButtonElement;
    expect(enable.disabled).toBe(false);
    await userEvent.click(enable);
    await waitFor(() =>
      expect(mocks.configureIssueIntakePolicy).toHaveBeenCalledWith(
        "project-1",
        expect.objectContaining({
          repository_full_name: "acme/widgets",
          reason: "política R0 deliberada y acotada",
          acknowledge: true,
        }),
      ),
    );
    expect(
      await screen.findByText(/Revisión de la política de admisión reconocida/),
    ).toBeTruthy();
    expect(onRefresh).toHaveBeenCalled();
  });

  it("acknowledges a Spanish batch selection and clears only after success", async () => {
    const onRefresh = vi.fn();
    const onSelectionClear = vi.fn();
    mocks.createIssueIntakeBatch.mockResolvedValue({
      batch: { id: "batch-1" },
      items: [],
      created: true,
    });
    renderPanel({
      locale: "es",
      selected: [issue],
      onRefresh,
      onSelectionClear,
    });
    expect(
      screen.getByText(
        /1 instantánea\(s\) seleccionada\(s\)\. Coste máximo reconocido/,
      ),
    ).toBeTruthy();
    expect(screen.getByText("1 SELECCIONADAS")).toBeTruthy();
    await userEvent.type(
      screen.getByLabelText("Motivo del lote"),
      "instantánea exacta seleccionada",
    );
    await userEvent.click(
      screen.getByLabelText(/Reconozco estos ID de incidencia/),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Reconocer lote finito" }),
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
    expect(
      await screen.findByText(/Reconocimiento registrado: 1 instantánea/),
    ).toBeTruthy();
    expect(onSelectionClear).toHaveBeenCalled();
    expect(onRefresh).toHaveBeenCalled();
  });

  it("renders Spanish queue states while keeping Core codes raw", () => {
    renderPanel({
      locale: "es",
      state: state({ batches: [activeBatchState()], queue: [queuedItem()] }),
    });
    expect(
      screen.getByRole("heading", { name: "Cola de conciliación duradera" }),
    ).toBeTruthy();
    expect(screen.getByText(/REVIEW_PLAN/)).toBeTruthy();
    expect(screen.getByText("intento 0/3")).toBeTruthy();
    expect(screen.getByText("ACTIVE")).toBeTruthy();
    expect(screen.getAllByText("PENDING").length).toBeGreaterThan(0);
    expect(
      screen.getByRole("button", { name: "Crear propuesta" }),
    ).toBeTruthy();
  });

  it("keeps eligibility reason codes raw in Spanish", () => {
    const batchState = activeBatchState();
    batchState.items[0].current_eligibility = {
      eligible: false,
      reasons: ["CONNECTION_REVISION_CHANGED"],
    };
    renderPanel({
      locale: "es",
      state: state({ batches: [batchState] }),
    });
    expect(screen.getByText("BLOQUEADA")).toBeTruthy();
    expect(screen.getByText("CONNECTION_REVISION_CHANGED")).toBeTruthy();
    expect(screen.getByText("PENDING")).toBeTruthy();
  });

  it("reports the Spanish create-proposal confirmation without translating the mode", async () => {
    const batchState = activeBatchState();
    const queued = queuedItem();
    mocks.dispatchIssueIntake.mockResolvedValue({
      status: "RECONCILED",
      reasons: [],
      queue: {
        ...queued,
        status: "RECONCILED",
        intake_request_id: "intake-1",
        lead_request_id: "lead-1",
      },
      intake: {
        id: "intake-1",
        lead_request_id: "lead-1",
        action_id: "action-1",
        objective: "Resolve issue",
        mode: "REVIEW_PLAN",
      },
      campaign_started: false,
      provider_calls: 0,
    });
    renderPanel({
      locale: "es",
      state: state({ batches: [batchState], queue: [queued] }),
    });
    await userEvent.click(
      screen.getByRole("button", { name: "Crear propuesta" }),
    );
    await waitFor(() =>
      expect(mocks.dispatchIssueIntake).toHaveBeenCalledWith(
        "project-1",
        "batch-1",
      ),
    );
    expect(
      await screen.findByText(
        /Propuesta intake-1 creada como REVIEW_PLAN\. No se inició ninguna Campaña/,
      ),
    ).toBeTruthy();
    expect(
      screen.getByText(/no se realizó ninguna llamada al proveedor/),
    ).toBeTruthy();
  });
});
