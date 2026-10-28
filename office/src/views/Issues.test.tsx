import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CoreApiError,
  type GitHubConnectionState,
  type GitHubIssueDetail,
  type GitHubIssueList,
  type GitHubRepositoryList,
  type GitHubSyncState,
  type Project,
  type ProjectGitHubBinding,
} from "../coreApi";
import { I18nProvider, type Locale } from "../i18n";
import { Issues } from "./Issues";

const mocks = vi.hoisted(() => ({
  listProjects: vi.fn(),
  getGitHubConnection: vi.fn(),
  updateGitHubConnection: vi.fn(),
  disconnectGitHub: vi.fn(),
  listGitHubRepositories: vi.fn(),
  bindProjectGitHub: vi.fn(),
  syncProjectIssues: vi.fn(),
  listProjectIssues: vi.fn(),
  getProjectIssue: vi.fn(),
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

const project: Project = {
  id: "project-1",
  repository: "https://github.com/acme/widgets.git",
  base_commit: "0123456789abcdef0123456789abcdef01234567",
  created_at: "2026-10-27T10:00:00Z",
  identity: "identity-1",
};

const binding: ProjectGitHubBinding = {
  project_id: "project-1",
  owner: "acme",
  repo: "widgets",
  repo_full_name: "acme/widgets",
  default_branch: "main",
  bound_at: "2026-10-27T11:00:00Z",
  updated_at: "2026-10-27T11:00:00Z",
};

const connection: GitHubConnectionState = {
  connection: {
    configured: true,
    enabled: true,
    login: "octo",
    scopes: ["repo"],
    scopes_reported: true,
    credential_fingerprint: "0123456789abcdef",
    revision: 2,
    verified_at: "2026-10-27T10:30:00Z",
    revoked_at: null,
    updated_at: "2026-10-27T10:30:00Z",
    detail:
      "credential verified; repository access and inference are not implied",
  },
  audit: [],
};

const syncState: GitHubSyncState = {
  project_id: "project-1",
  repo_full_name: "acme/widgets",
  status: "OK",
  detail: "",
  etag: '"etag-1"',
  last_synced_at: "2026-10-27T12:00:00Z",
  last_attempt_at: "2026-10-27T12:00:00Z",
  next_allowed_at: null,
  last_status_code: 200,
  fetched: 2,
  created: 2,
  updated: 0,
  removed: 0,
  total_known: 2,
  truncated: false,
  stale: false,
  updated_at: "2026-10-27T12:00:00Z",
};

function issueList(overrides: Partial<GitHubIssueList> = {}): GitHubIssueList {
  return {
    items: [
      {
        number: 1,
        title: "Export CSV endpoint",
        state: "open",
        state_reason: null,
        labels: [{ name: "enhancement", color: "00ff00" }],
        author: "octo",
        html_url: "https://github.com/acme/widgets/issues/1",
        comments: 3,
        created_at: "2026-10-26T10:00:00Z",
        updated_at: "2026-10-27T09:00:00Z",
        closed_at: null,
        synced_at: "2026-10-27T12:00:00Z",
      },
      {
        number: 2,
        title: "Flaky integration test",
        state: "open",
        state_reason: null,
        labels: [{ name: "bug", color: "ff0000" }],
        author: "maintainer",
        html_url: "https://github.com/acme/widgets/issues/2",
        comments: 0,
        created_at: "2026-10-25T10:00:00Z",
        updated_at: "2026-10-26T09:00:00Z",
        closed_at: null,
        synced_at: "2026-10-27T12:00:00Z",
      },
    ],
    total: 2,
    limit: 25,
    offset: 0,
    synced_at: "2026-10-27T12:00:00Z",
    stale: false,
    sync_status: "OK",
    last_error: null,
    next_allowed_at: null,
    truncated: false,
    binding,
    ...overrides,
  };
}

const detail: GitHubIssueDetail = {
  ...issueList().items[0],
  body: "Please build the export.\n\n<script>alert('untrusted')</script>\n\nDo not run this.",
  body_truncated: false,
};

function renderIssues(path = "/issues?project=project-1", locale?: Locale) {
  const content = (
    <MemoryRouter initialEntries={[path]}>
      <Issues />
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

beforeEach(() => {
  mocks.listProjects.mockReset().mockResolvedValue([project]);
  mocks.getGitHubConnection.mockReset().mockResolvedValue(connection);
  mocks.updateGitHubConnection.mockReset();
  mocks.disconnectGitHub.mockReset();
  mocks.listGitHubRepositories.mockReset();
  mocks.bindProjectGitHub.mockReset();
  mocks.syncProjectIssues.mockReset().mockResolvedValue(syncState);
  mocks.listProjectIssues.mockReset().mockResolvedValue(issueList());
  mocks.getProjectIssue.mockReset().mockResolvedValue(detail);
});

describe("Issues", () => {
  it("renders the Core-backed connection, snapshot and issue detail as plain text", async () => {
    const { container } = renderIssues();
    expect(
      await screen.findByRole("heading", { name: "Issues" }),
    ).toBeDefined();
    expect(await screen.findByText("CONNECTED")).toBeDefined();
    expect(screen.getByText("SYNCED")).toBeDefined();
    expect(screen.getByText("acme/widgets")).toBeDefined();
    expect(
      await screen.findByRole("button", { name: "Export CSV endpoint" }),
    ).toBeDefined();
    expect(screen.getByText("Showing 1–2 of 2")).toBeDefined();
    expect(mocks.listProjectIssues).toHaveBeenCalledWith(
      "project-1",
      expect.objectContaining({ state: "open", limit: 25, offset: 0 }),
    );

    await userEvent.click(
      screen.getByRole("button", { name: "Export CSV endpoint" }),
    );
    expect(
      await screen.findByRole("heading", {
        name: /#1 Export CSV endpoint/,
      }),
    ).toBeDefined();
    expect(screen.getByText(/Please build the export/)).toBeDefined();
    expect(container.querySelector("script")).toBeNull();
    expect(mocks.getProjectIssue).toHaveBeenCalledWith("project-1", 1);
  });

  it("renders the connection, filters and issue table in Spanish", async () => {
    renderIssues("/issues?project=project-1", "es");
    expect(
      await screen.findByRole("heading", { name: "Incidencias" }),
    ).toBeDefined();
    expect(screen.getByText("Conexión con GitHub")).toBeDefined();
    expect(screen.getByText("Proyecto y repositorio")).toBeDefined();
    expect(screen.getByText("Incidencias sincronizadas")).toBeDefined();
    expect(
      screen.getByRole("button", { name: "Sincronizar incidencias" }),
    ).toBeDefined();
    const title = await screen.findByRole("button", {
      name: "Export CSV endpoint",
    });
    expect(screen.getByText("Mostrando 1–2 de 2")).toBeDefined();
    expect(title.closest("td")?.getAttribute("data-label")).toBe("Título");
    expect(screen.getAllByText("open").length).toBeGreaterThan(0);
    expect(screen.getByText("CONNECTED")).toBeDefined();
  });

  it("shows a truthful empty state and runs an explicit sync", async () => {
    mocks.listProjectIssues.mockResolvedValue(
      issueList({
        items: [],
        total: 0,
        synced_at: null,
        sync_status: "NEVER_SYNCED",
        stale: true,
      }),
    );
    renderIssues();
    expect(
      await screen.findByText("No issues have been synchronized"),
    ).toBeDefined();
    const syncButtons = screen.getAllByRole("button", { name: "Sync issues" });
    await userEvent.click(syncButtons[0]);
    await waitFor(() =>
      expect(mocks.syncProjectIssues).toHaveBeenCalledWith("project-1"),
    );
    expect(await screen.findByText(/Sync completed: 2 fetched/)).toBeDefined();
  });

  it("surfaces snapshot errors without inventing data", async () => {
    mocks.listProjectIssues.mockRejectedValue(
      new CoreApiError(500, "database unavailable"),
    );
    renderIssues();
    expect(await screen.findByText("Issue snapshot unavailable")).toBeDefined();
    expect(screen.getByText("database unavailable")).toBeDefined();
    expect(screen.queryByText("Export CSV endpoint")).toBeNull();
  });

  it("keeps a revoked connection and stale snapshot visibly disabled", async () => {
    mocks.getGitHubConnection.mockResolvedValue({
      connection: {
        ...connection.connection,
        configured: false,
        enabled: false,
        credential_fingerprint: "",
        revoked_at: "2026-10-27T13:00:00Z",
        detail: "connection revoked; synchronization is disabled",
      },
      audit: [
        {
          audit_id: "audit-1",
          revision: 3,
          action: "revoked",
          actor: "office-operator",
          detail: "credential cleared; synchronization disabled",
          created_at: "2026-10-27T13:00:00Z",
        },
      ],
    });
    mocks.listProjectIssues.mockResolvedValue(
      issueList({
        stale: true,
        sync_status: "REVOKED",
        last_error:
          "GitHub connection was revoked; synchronization is disabled",
      }),
    );
    renderIssues();
    expect((await screen.findAllByText("REVOKED")).length).toBeGreaterThan(0);
    expect(screen.getByText("STALE")).toBeDefined();
    expect(
      screen.getByText(
        "GitHub connection was revoked; synchronization is disabled",
      ),
    ).toBeDefined();
    expect(
      (
        screen.getByRole("button", {
          name: "Sync issues",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(screen.getByLabelText("GitHub token")).toBeDefined();
    expect(
      screen.getByText("connection revoked; synchronization is disabled"),
    ).toBeDefined();
  });

  it("reports a rate-limited sync and clears the token field after connection", async () => {
    mocks.getGitHubConnection.mockResolvedValue({
      connection: {
        ...connection.connection,
        configured: false,
        enabled: false,
      },
      audit: [],
    });
    mocks.updateGitHubConnection.mockResolvedValue(connection);
    renderIssues();
    const token = await screen.findByLabelText("GitHub token");
    await userEvent.type(token, "ghp_browser_test_token_1234567890");
    await userEvent.click(
      screen.getByRole("button", { name: "Verify and save" }),
    );
    await waitFor(() =>
      expect(mocks.updateGitHubConnection).toHaveBeenCalledWith(
        "ghp_browser_test_token_1234567890",
      ),
    );
    await waitFor(() =>
      expect(
        (screen.getByLabelText("GitHub token") as HTMLInputElement).value,
      ).toBe(""),
    );
    expect(
      screen.getByText(/GitHub credential verified for octo/),
    ).toBeDefined();

    mocks.syncProjectIssues.mockResolvedValue({
      ...syncState,
      status: "RATE_LIMITED",
      detail: "GitHub rate limit is exhausted until 2026-10-27T13:30:00Z.",
      next_allowed_at: "2026-10-27T13:30:00Z",
      stale: true,
    });
    const syncButtons = screen.getAllByRole("button", { name: "Sync issues" });
    await userEvent.click(syncButtons[0]);
    expect(
      await screen.findByText(/GitHub rate limit is exhausted/),
    ).toBeDefined();
    expect(screen.getByText(/Rate limit retry after/)).toBeDefined();
    expect(mocks.listProjectIssues).toHaveBeenCalled();
  });

  it("disables sync during a shared rate-limit backoff and re-enables after it", async () => {
    mocks.listProjectIssues.mockResolvedValue(
      issueList({
        stale: true,
        sync_status: "RATE_LIMITED",
        next_allowed_at: "2099-01-01T00:00:00Z",
        last_error:
          "GitHub rate limit is exhausted until 2099-01-01T00:00:00Z.",
      }),
    );
    const { unmount } = renderIssues();
    expect(await screen.findByText("BACKOFF ACTIVE")).toBeDefined();
    expect(
      (
        screen.getByRole("button", {
          name: "Sync issues",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(
      screen.getByText(/Synchronization is disabled for every Project/),
    ).toBeDefined();
    unmount();

    mocks.listProjectIssues.mockResolvedValue(
      issueList({
        stale: false,
        sync_status: "RATE_LIMITED",
        next_allowed_at: "2000-01-01T00:00:00Z",
      }),
    );
    renderIssues();
    await screen.findByText("Export CSV endpoint");
    expect(
      (
        screen.getByRole("button", {
          name: "Sync issues",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
  });

  it("selects a visible private repository into the binding form", async () => {
    const repos: GitHubRepositoryList = {
      repositories: [
        {
          full_name: "acme/private-widgets",
          private: true,
          default_branch: "develop",
          html_url: "https://github.com/acme/private-widgets",
          pushed_at: "2026-10-27T10:00:00Z",
        },
      ],
      fetched_at: "2026-10-27T12:00:00Z",
      rate_limited: false,
      next_allowed_at: null,
      detail: "",
    };
    mocks.listProjectIssues.mockResolvedValue(
      issueList({ binding: null, items: [], total: 0, synced_at: null }),
    );
    mocks.listGitHubRepositories.mockResolvedValue(repos);
    renderIssues();
    await screen.findByText("No repository is bound");
    await userEvent.click(
      screen.getByRole("button", { name: "Browse GitHub repositories" }),
    );
    const select = await screen.findByLabelText("Visible repository");
    await userEvent.selectOptions(select, "acme/private-widgets");
    expect((screen.getByLabelText("Owner") as HTMLInputElement).value).toBe(
      "acme",
    );
    expect(
      (screen.getByLabelText("Repository") as HTMLInputElement).value,
    ).toBe("private-widgets");
    expect(
      (screen.getByLabelText("Default branch") as HTMLInputElement).value,
    ).toBe("develop");
  });
});
