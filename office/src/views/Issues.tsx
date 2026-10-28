import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { Badge, type BadgeTone } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Input, Select } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  CoreApiError,
  coreApi,
  type GitHubConnectionState,
  type GitHubIssueDetail,
  type GitHubIssueList,
  type GitHubIssueSummary,
  type GitHubRepositoryList,
  type GitHubSyncState,
  type IssueIntakeState,
  type Project,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";
import { IssueIntakePanel } from "./IssueIntakePanel";
import "./issues.css";

const PAGE_SIZE = 25;

type IssueStateFilter = "all" | "open" | "closed";

function message(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function syncBadge(status: GitHubIssueList["sync_status"]): {
  tone: BadgeTone;
  label: string;
} {
  switch (status) {
    case "OK":
      return { tone: "success", label: "SYNCED" };
    case "STALE":
      return { tone: "attention", label: "STALE" };
    case "RATE_LIMITED":
      return { tone: "attention", label: "RATE LIMITED" };
    case "DENIED":
      return { tone: "danger", label: "DENIED" };
    case "NOT_FOUND":
      return { tone: "danger", label: "NOT FOUND" };
    case "ERROR":
      return { tone: "danger", label: "SYNC ERROR" };
    case "REVOKED":
      return { tone: "danger", label: "REVOKED" };
    case "UNBOUND":
      return { tone: "attention", label: "UNBOUND" };
    default:
      return { tone: "neutral", label: "NEVER SYNCED" };
  }
}

function issueStateBadge(state: GitHubIssueSummary["state"]) {
  return state === "open" ? (
    <Badge tone="success">open</Badge>
  ) : (
    <Badge tone="muted">closed</Badge>
  );
}

function isSafeHttpsUrl(value: string): boolean {
  return value.startsWith("https://");
}

function laterDeadline(
  first: string | null | undefined,
  second: string | null | undefined,
): string | null {
  const candidates = [first, second].filter(
    (value): value is string => typeof value === "string" && value !== "",
  );
  let latest: { value: string; millis: number } | null = null;
  for (const value of candidates) {
    const millis = Date.parse(value);
    if (!Number.isFinite(millis)) continue;
    if (latest === null || millis > latest.millis) {
      latest = { value, millis };
    }
  }
  return latest?.value ?? null;
}

export function Issues() {
  const [searchParams, setSearchParams] = useSearchParams();
  const projectId = searchParams.get("project") ?? "";
  const issueNumber = searchParams.get("issue") ?? "";
  const stateParam = searchParams.get("state");
  const stateFilter: IssueStateFilter =
    stateParam === "all" || stateParam === "closed" ? stateParam : "open";
  const labelFilter = searchParams.get("label") ?? "";
  const appliedQuery = searchParams.get("q") ?? "";
  const offsetValue = Number.parseInt(searchParams.get("offset") ?? "0", 10);
  const offset =
    Number.isFinite(offsetValue) && offsetValue >= 0 ? offsetValue : 0;

  const [projects, setProjects] = useState<Project[]>([]);
  const [projectsLoading, setProjectsLoading] = useState(true);
  const [connectionState, setConnectionState] =
    useState<GitHubConnectionState | null>(null);
  const [connectionLoading, setConnectionLoading] = useState(true);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [issues, setIssues] = useState<GitHubIssueList | null>(null);
  const [issuesLoading, setIssuesLoading] = useState(false);
  const [issuesError, setIssuesError] = useState<string | null>(null);
  const [detail, setDetail] = useState<GitHubIssueDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [lastSync, setLastSync] = useState<GitHubSyncState | null>(null);
  const [intake, setIntake] = useState<IssueIntakeState | null>(null);
  const [intakeLoading, setIntakeLoading] = useState(false);
  const [intakeError, setIntakeError] = useState<string | null>(null);
  const [selectedIssues, setSelectedIssues] = useState<
    Record<number, GitHubIssueSummary>
  >({});
  const [repositories, setRepositories] = useState<GitHubRepositoryList | null>(
    null,
  );
  const [repositoryPick, setRepositoryPick] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);
  const [bindingOpen, setBindingOpen] = useState(false);
  const [bindingOwner, setBindingOwner] = useState("");
  const [bindingRepo, setBindingRepo] = useState("");
  const [bindingBranch, setBindingBranch] = useState("main");
  const [labelInput, setLabelInput] = useState(labelFilter);
  const [queryInput, setQueryInput] = useState(appliedQuery);
  const [refresh, setRefresh] = useState(0);
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    setLabelInput(labelFilter);
  }, [labelFilter]);

  useEffect(() => {
    const timer = window.setInterval(() => setNowMs(Date.now()), 15_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    setQueryInput(appliedQuery);
  }, [appliedQuery]);

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [nextProjects, nextConnection] = await Promise.all([
          coreApi.listProjects(),
          coreApi.getGitHubConnection(),
        ]);
        if (!active) return;
        setProjects(nextProjects);
        setConnectionState(nextConnection);
      } catch (cause) {
        if (active) {
          setConnectionError(
            message(cause, "Failed to load GitHub integration state."),
          );
        }
      } finally {
        if (active) {
          setProjectsLoading(false);
          setConnectionLoading(false);
        }
      }
    }
    void load();
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!projectId && projects.length > 0) {
      setSearchParams(
        (current) => {
          const next = new URLSearchParams(current);
          next.set("project", projects[0].id);
          return next;
        },
        { replace: true },
      );
    }
  }, [projectId, projects, setSearchParams]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: projectId is an invalidation key for Project-scoped UI state.
  useEffect(() => {
    setLastSync(null);
    setIntake(null);
    setIntakeError(null);
    setSelectedIssues({});
    setIssues(null);
    setIssuesError(null);
    setRepositories(null);
    setRepositoryPick("");
    setBindingOwner("");
    setBindingRepo("");
    setBindingBranch("main");
    setBindingOpen(false);
    setConfirmDisconnect(false);
  }, [projectId]);

  const binding = issues?.binding ?? null;

  useEffect(() => {
    if (!binding) return;
    setBindingOwner(binding.owner);
    setBindingRepo(binding.repo);
    setBindingBranch(binding.default_branch);
  }, [binding]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: refresh is an explicit reload token.
  useEffect(() => {
    if (!projectId) {
      setIssues(null);
      setIssuesError(null);
      return;
    }
    let active = true;
    setIssuesLoading(true);
    setIssuesError(null);
    coreApi
      .listProjectIssues(projectId, {
        state: stateFilter,
        label: labelFilter || undefined,
        query: appliedQuery || undefined,
        limit: PAGE_SIZE,
        offset,
      })
      .then((next) => {
        if (active) setIssues(next);
      })
      .catch((cause) => {
        if (active) {
          setIssuesError(
            message(cause, "Failed to load the synchronized issue snapshot."),
          );
        }
      })
      .finally(() => {
        if (active) setIssuesLoading(false);
      });
    return () => {
      active = false;
    };
  }, [projectId, stateFilter, labelFilter, appliedQuery, offset, refresh]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: refresh is an explicit reload token.
  useEffect(() => {
    if (!projectId) {
      setIntake(null);
      setIntakeError(null);
      return;
    }
    let active = true;
    setIntakeLoading(true);
    setIntakeError(null);
    coreApi
      .getIssueIntake(projectId)
      .then((next) => {
        if (active) setIntake(next);
      })
      .catch((cause) => {
        if (active) {
          setIntakeError(
            message(cause, "Failed to load the issue-intake policy."),
          );
        }
      })
      .finally(() => {
        if (active) setIntakeLoading(false);
      });
    return () => {
      active = false;
    };
  }, [projectId, refresh]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: refresh is an explicit reload token.
  useEffect(() => {
    if (!projectId || !issueNumber) {
      setDetail(null);
      setDetailError(null);
      return;
    }
    const number = Number.parseInt(issueNumber, 10);
    if (!Number.isInteger(number) || number <= 0) {
      setDetail(null);
      setDetailError("Issue number is invalid.");
      return;
    }
    let active = true;
    setDetailLoading(true);
    setDetailError(null);
    coreApi
      .getProjectIssue(projectId, number)
      .then((next) => {
        if (active) setDetail(next);
      })
      .catch((cause) => {
        if (active) {
          setDetail(null);
          setDetailError(
            cause instanceof CoreApiError && cause.status === 404
              ? "Issue is no longer in the synchronized snapshot."
              : message(cause, "Failed to load the issue detail."),
          );
        }
      })
      .finally(() => {
        if (active) setDetailLoading(false);
      });
    return () => {
      active = false;
    };
  }, [projectId, issueNumber, refresh]);

  function updateParams(mutate: (params: URLSearchParams) => void) {
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        mutate(next);
        return next;
      },
      { replace: true },
    );
  }

  function selectProject(nextProjectId: string) {
    updateParams((params) => {
      if (nextProjectId) params.set("project", nextProjectId);
      else params.delete("project");
      params.delete("issue");
      params.delete("label");
      params.delete("q");
      params.delete("state");
      params.delete("offset");
    });
    setActionError(null);
    setNotice(null);
  }

  function selectState(nextState: IssueStateFilter) {
    updateParams((params) => {
      if (nextState === "open") params.delete("state");
      else params.set("state", nextState);
      params.delete("offset");
    });
  }

  function applyFilters(event: React.FormEvent) {
    event.preventDefault();
    updateParams((params) => {
      const label = labelInput.trim();
      const query = queryInput.trim();
      if (label) params.set("label", label);
      else params.delete("label");
      if (query) params.set("q", query);
      else params.delete("q");
      params.delete("offset");
    });
  }

  function clearFilters() {
    setLabelInput("");
    setQueryInput("");
    updateParams((params) => {
      params.delete("label");
      params.delete("q");
      params.delete("state");
      params.delete("offset");
    });
  }

  function openIssue(number: number) {
    updateParams((params) => {
      params.set("issue", String(number));
    });
  }

  function closeIssue() {
    updateParams((params) => {
      params.delete("issue");
    });
  }

  function changeOffset(nextOffset: number) {
    updateParams((params) => {
      if (nextOffset > 0) params.set("offset", String(nextOffset));
      else params.delete("offset");
    });
  }

  async function onConnect(event: React.FormEvent) {
    event.preventDefault();
    if (token.trim() === "") return;
    setBusy("connect");
    setActionError(null);
    setNotice(null);
    try {
      const next = await coreApi.updateGitHubConnection(token);
      setConnectionState(next);
      setToken("");
      setConfirmDisconnect(false);
      setNotice(
        `GitHub credential verified for ${next.connection.login ?? "the token"}. Repository access and inference are not implied.`,
      );
      setRefresh((value) => value + 1);
    } catch (cause) {
      setActionError(message(cause, "GitHub credential verification failed."));
    } finally {
      setBusy(null);
    }
  }

  async function onDisconnect() {
    setBusy("disconnect");
    setActionError(null);
    setNotice(null);
    try {
      const next = await coreApi.disconnectGitHub();
      setConnectionState(next);
      setConfirmDisconnect(false);
      setRepositories(null);
      setRepositoryPick("");
      setNotice(
        "GitHub connection revoked. The encrypted credential was cleared and synchronization is disabled.",
      );
      setRefresh((value) => value + 1);
    } catch (cause) {
      setActionError(message(cause, "GitHub disconnection failed."));
    } finally {
      setBusy(null);
    }
  }

  async function onLoadRepositories() {
    setBusy("repositories");
    setActionError(null);
    setNotice(null);
    try {
      const next = await coreApi.listGitHubRepositories();
      setRepositories(next);
      if (next.rate_limited) {
        setNotice(
          next.next_allowed_at
            ? `GitHub rate limit reached. Retry after ${formatTimestamp(next.next_allowed_at)}.`
            : next.detail,
        );
      } else if (next.repositories.length === 0) {
        setNotice(
          "GitHub returned no repositories visible to this credential.",
        );
      }
    } catch (cause) {
      setActionError(
        message(cause, "GitHub repositories could not be loaded."),
      );
    } finally {
      setBusy(null);
    }
  }

  function onPickRepository(fullName: string) {
    setRepositoryPick(fullName);
    const repository = repositories?.repositories.find(
      (item) => item.full_name === fullName,
    );
    if (!repository) return;
    const [owner, repo] = repository.full_name.split("/");
    setBindingOwner(owner ?? "");
    setBindingRepo(repo ?? "");
    setBindingBranch(repository.default_branch || "main");
  }

  async function onBind(event: React.FormEvent) {
    event.preventDefault();
    if (!projectId) return;
    setBusy("bind");
    setActionError(null);
    setNotice(null);
    try {
      const selectedRepository = repositories?.repositories.find(
        (repository) => repository.full_name === repositoryPick,
      );
      const result = await coreApi.bindProjectGitHub(projectId, {
        owner: bindingOwner.trim(),
        repo: bindingRepo.trim(),
        default_branch: bindingBranch.trim(),
        repository_github_id: selectedRepository?.id ?? null,
      });
      setLastSync(result.sync_state);
      setBindingOpen(false);
      setNotice(
        `Bound ${result.binding.repo_full_name} at branch ${result.binding.default_branch}. Run a sync to import issues.`,
      );
      setRefresh((value) => value + 1);
    } catch (cause) {
      setActionError(message(cause, "GitHub repository binding failed."));
    } finally {
      setBusy(null);
    }
  }

  async function onSync() {
    if (!projectId) return;
    setBusy("sync");
    setActionError(null);
    setNotice(null);
    try {
      const result = await coreApi.syncProjectIssues(projectId);
      setLastSync(result);
      setRefresh((value) => value + 1);
      if (result.status === "OK") {
        setNotice(
          `Sync completed: ${result.fetched} fetched, ${result.created} created, ${result.updated} updated, ${result.removed} removed.`,
        );
      } else {
        setActionError(result.detail || `Sync finished with ${result.status}.`);
      }
    } catch (cause) {
      setActionError(message(cause, "GitHub issue synchronization failed."));
      setRefresh((value) => value + 1);
    } finally {
      setBusy(null);
    }
  }

  const connection = connectionState?.connection ?? null;
  const syncStatus = lastSync?.status ?? issues?.sync_status ?? "NEVER_SYNCED";
  const statusBadge = syncBadge(syncStatus);
  const nextAllowed = laterDeadline(
    lastSync?.next_allowed_at,
    issues?.next_allowed_at,
  );
  const backoffActive = nextAllowed
    ? new Date(nextAllowed).getTime() > nowMs
    : false;
  const canSync =
    projectId !== "" &&
    binding !== null &&
    connection?.configured === true &&
    busy === null &&
    !backoffActive;
  function toggleIssueSelection(issue: GitHubIssueSummary) {
    setSelectedIssues((current) => {
      const next = { ...current };
      if (next[issue.number]) delete next[issue.number];
      else next[issue.number] = issue;
      return next;
    });
  }

  const selectedList = Object.values(selectedIssues);
  const intakeEnabled = intake?.policy?.enabled === true;
  const total = issues?.total ?? 0;
  const showingFrom = total === 0 ? 0 : offset + 1;
  const showingTo = Math.min(offset + PAGE_SIZE, total);

  return (
    <div className="issues-view">
      <PageHeader
        title="Issues"
        description="Synchronized GitHub issue snapshots for a bound Project. Issue text is untrusted and shown as plain text; Core never treats it as instructions."
        meta={
          <>
            <span>Core-owned snapshots</span>
            <span>Manual synchronization</span>
            <span>No agent execution</span>
          </>
        }
        actions={
          <>
            <Button
              variant="quiet"
              onClick={() => setRefresh((value) => value + 1)}
              disabled={busy !== null}
            >
              Refresh
            </Button>
            <Button
              variant="primary"
              onClick={() => void onSync()}
              disabled={!canSync}
              title={
                backoffActive
                  ? `GitHub rate-limit backoff until ${formatTimestamp(nextAllowed ?? "")}`
                  : binding === null
                    ? "Bind a GitHub repository first"
                    : connection?.configured
                      ? undefined
                      : "Connect GitHub first"
              }
            >
              {busy === "sync" ? "Syncing…" : "Sync issues"}
            </Button>
          </>
        }
      />

      {connectionError && (
        <ErrorState
          title="GitHub integration unavailable"
          message={connectionError}
          action={
            <Button onClick={() => setRefresh((value) => value + 1)}>
              Retry
            </Button>
          }
        />
      )}
      {actionError && (
        <p className="error" role="alert">
          {actionError}
        </p>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      <Panel className="issues-panel">
        <div className="issues-panel-heading">
          <h2>GitHub connection</h2>
          {connection?.configured ? (
            <Badge tone="success">CONNECTED</Badge>
          ) : connection?.revoked_at ? (
            <Badge tone="danger">REVOKED</Badge>
          ) : (
            <Badge tone="neutral">NOT CONNECTED</Badge>
          )}
        </div>
        {connectionLoading ? (
          <LoadingState label="Loading GitHub connection…" />
        ) : (
          <>
            <p className="muted">
              {connection?.detail ??
                "Core stores the GitHub token encrypted and never returns it to Office."}
            </p>
            {connection?.login && (
              <dl className="issues-facts">
                <div>
                  <dt>Authenticated login</dt>
                  <dd>{connection.login}</dd>
                </div>
                <div>
                  <dt>Token scopes</dt>
                  <dd>
                    {connection.scopes_reported
                      ? connection.scopes.length > 0
                        ? connection.scopes.join(", ")
                        : "none reported"
                      : "not reported by this token type"}
                  </dd>
                </div>
                <div>
                  <dt>Verified at</dt>
                  <dd>{formatTimestamp(connection.verified_at ?? "")}</dd>
                </div>
                <div>
                  <dt>Credential fingerprint</dt>
                  <dd className="mono">
                    {connection.credential_fingerprint || "—"}
                  </dd>
                </div>
                <div>
                  <dt>Revision</dt>
                  <dd>{connection.revision}</dd>
                </div>
              </dl>
            )}
            <p className="issues-note">
              A verified token proves authentication only. It does not prove
              repository access, inference, quota or delivery authority.
            </p>
            {connection?.configured && (
              <details className="issues-details">
                <summary>Replace token</summary>
                <form className="issues-inline-form" onSubmit={onConnect}>
                  <Input
                    label="GitHub token"
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    value={token}
                    onChange={(event) => setToken(event.target.value)}
                    help="Stored encrypted by Core. Never written to browser storage and never returned by Core."
                    required
                  />
                  <Button
                    type="submit"
                    variant="primary"
                    disabled={busy !== null || token.trim() === ""}
                  >
                    {busy === "connect" ? "Verifying…" : "Verify and save"}
                  </Button>
                </form>
              </details>
            )}
            {!connection?.configured && (
              <form className="issues-inline-form" onSubmit={onConnect}>
                <Input
                  label="GitHub token"
                  type="password"
                  autoComplete="off"
                  spellCheck={false}
                  value={token}
                  onChange={(event) => setToken(event.target.value)}
                  help="Use a fine-grained or classic token with repository Issues read access. Core verifies it against GitHub, stores it encrypted and never returns it."
                  required
                />
                <Button
                  type="submit"
                  variant="primary"
                  disabled={busy !== null || token.trim() === ""}
                >
                  {busy === "connect" ? "Verifying…" : "Verify and save"}
                </Button>
              </form>
            )}
            {connection?.login && (
              <div className="issues-actions">
                {confirmDisconnect ? (
                  <fieldset className="issues-confirm">
                    <legend className="issues-confirm-legend">
                      Confirm GitHub disconnect
                    </legend>
                    <p>
                      Disconnecting clears the encrypted credential, disables
                      the connection and stops every synchronization.
                    </p>
                    <Button
                      variant="danger"
                      onClick={() => void onDisconnect()}
                      disabled={busy !== null}
                    >
                      {busy === "disconnect"
                        ? "Disconnecting…"
                        : "Confirm disconnect"}
                    </Button>
                    <Button
                      variant="quiet"
                      onClick={() => setConfirmDisconnect(false)}
                      disabled={busy !== null}
                    >
                      Cancel
                    </Button>
                  </fieldset>
                ) : (
                  <Button
                    variant="danger"
                    onClick={() => setConfirmDisconnect(true)}
                    disabled={busy !== null}
                  >
                    Disconnect GitHub
                  </Button>
                )}
              </div>
            )}
            {connectionState && connectionState.audit.length > 0 && (
              <details className="issues-details">
                <summary>
                  Connection audit ({connectionState.audit.length})
                </summary>
                <ul className="issues-audit">
                  {connectionState.audit.map((entry) => (
                    <li key={entry.audit_id}>
                      <strong>{entry.action}</strong> revision {entry.revision}{" "}
                      by {entry.actor} — {formatTimestamp(entry.created_at)}
                      {entry.detail ? ` — ${entry.detail}` : ""}
                    </li>
                  ))}
                </ul>
              </details>
            )}
          </>
        )}
      </Panel>

      <Panel className="issues-panel">
        <div className="issues-panel-heading">
          <h2>Project and repository</h2>
          <Badge tone={binding ? "info" : "neutral"}>
            {binding ? "BOUND" : "UNBOUND"}
          </Badge>
        </div>
        {projectsLoading ? (
          <LoadingState label="Loading Projects…" />
        ) : projects.length === 0 ? (
          <EmptyState
            title="No Projects are registered"
            description="Register a Project before binding a GitHub repository."
          />
        ) : (
          <>
            <Select
              label="Project"
              value={projectId}
              onChange={(event) => selectProject(event.target.value)}
            >
              <option value="">Select a Project</option>
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.repository} ({shortId(project.id)})
                </option>
              ))}
            </Select>
            {binding ? (
              <dl className="issues-facts">
                <div>
                  <dt>Repository</dt>
                  <dd className="mono">{binding.repo_full_name}</dd>
                </div>
                <div>
                  <dt>Default branch</dt>
                  <dd className="mono">{binding.default_branch}</dd>
                </div>
                <div>
                  <dt>Bound at</dt>
                  <dd>{formatTimestamp(binding.bound_at)}</dd>
                </div>
              </dl>
            ) : (
              <p className="muted">
                This Project has no GitHub repository binding. Binding does not
                import issues until a sync runs.
              </p>
            )}
            {binding && !bindingOpen && (
              <Button
                onClick={() => setBindingOpen(true)}
                disabled={busy !== null}
              >
                Change binding
              </Button>
            )}
            {(!binding || bindingOpen) && (
              <form className="issues-bind-form" onSubmit={onBind}>
                <div className="issues-actions">
                  <Button
                    variant="quiet"
                    onClick={() => void onLoadRepositories()}
                    disabled={busy !== null || connection?.configured !== true}
                    title={
                      connection?.configured
                        ? undefined
                        : "Connect GitHub to browse repositories"
                    }
                  >
                    {busy === "repositories"
                      ? "Loading repositories…"
                      : "Browse GitHub repositories"}
                  </Button>
                  {repositories?.rate_limited && (
                    <Badge tone="attention">RATE LIMITED</Badge>
                  )}
                </div>
                {repositories && repositories.repositories.length > 0 && (
                  <Select
                    label="Visible repository"
                    value={repositoryPick}
                    onChange={(event) => onPickRepository(event.target.value)}
                    help="Private repositories are shown only when the token can access them."
                  >
                    <option value="">Select a repository…</option>
                    {repositories.repositories.map((repository) => (
                      <option
                        key={repository.full_name}
                        value={repository.full_name}
                      >
                        {repository.full_name}
                        {repository.private ? " (private)" : ""}
                      </option>
                    ))}
                  </Select>
                )}
                <div className="issues-bind-fields">
                  <Input
                    label="Owner"
                    value={bindingOwner}
                    onChange={(event) => setBindingOwner(event.target.value)}
                    placeholder="owner"
                    required
                  />
                  <Input
                    label="Repository"
                    value={bindingRepo}
                    onChange={(event) => setBindingRepo(event.target.value)}
                    placeholder="repository"
                    required
                  />
                  <Input
                    label="Default branch"
                    value={bindingBranch}
                    onChange={(event) => setBindingBranch(event.target.value)}
                    placeholder="main"
                    required
                  />
                </div>
                <div className="issues-actions">
                  <Button
                    type="submit"
                    variant="primary"
                    disabled={
                      busy !== null ||
                      projectId === "" ||
                      bindingOwner.trim() === "" ||
                      bindingRepo.trim() === "" ||
                      bindingBranch.trim() === ""
                    }
                  >
                    {busy === "bind" ? "Binding…" : "Bind repository"}
                  </Button>
                  {binding && (
                    <Button
                      variant="quiet"
                      onClick={() => {
                        setBindingOpen(false);
                        setBindingOwner(binding.owner);
                        setBindingRepo(binding.repo);
                        setBindingBranch(binding.default_branch);
                      }}
                      disabled={busy !== null}
                    >
                      Cancel
                    </Button>
                  )}
                </div>
              </form>
            )}
          </>
        )}
      </Panel>

      <Panel className="issues-panel">
        <div className="issues-panel-heading">
          <h2>Synchronized issues</h2>
          <Badge tone={statusBadge.tone}>{statusBadge.label}</Badge>
        </div>
        <div className="issues-sync-state">
          <span>
            {issues?.synced_at
              ? `Last successful sync ${formatTimestamp(issues.synced_at)}`
              : "Never synchronized"}
          </span>
          {issues?.stale && <Badge tone="attention">STALE</Badge>}
          {issues?.truncated && (
            <Badge tone="attention">BOUNDED SNAPSHOT</Badge>
          )}
          {nextAllowed && (
            <span>Rate limit retry after {formatTimestamp(nextAllowed)}</span>
          )}
          {backoffActive && <Badge tone="attention">BACKOFF ACTIVE</Badge>}
          {lastSync && (
            <span>
              Last run: fetched {lastSync.fetched}, created {lastSync.created},
              updated {lastSync.updated}, removed {lastSync.removed}
            </span>
          )}
        </div>
        {issues?.last_error && (
          <p className="error" role="alert">
            {issues.last_error}
          </p>
        )}
        {backoffActive && nextAllowed && (
          <p className="notice" role="status">
            GitHub rate-limit backoff is active until{" "}
            {formatTimestamp(nextAllowed)}. Synchronization is disabled for
            every Project sharing this credential.
          </p>
        )}
        {issues?.truncated && (
          <p className="issues-note">
            Only the first bounded issue pages are synchronized. Additional
            GitHub issues are not displayed and deletion reconciliation is
            suspended for this snapshot.
          </p>
        )}

        {projectId === "" ? (
          <EmptyState
            title="Select a Project"
            description="Choose a Project to inspect its GitHub issue snapshot."
          />
        ) : (
          <>
            <form className="issues-filters" onSubmit={applyFilters}>
              <Select
                label="State"
                value={stateFilter}
                onChange={(event) =>
                  selectState(event.target.value as IssueStateFilter)
                }
              >
                <option value="open">Open</option>
                <option value="closed">Closed</option>
                <option value="all">All states</option>
              </Select>
              <Input
                label="Label"
                value={labelInput}
                onChange={(event) => setLabelInput(event.target.value)}
                placeholder="exact label"
              />
              <Input
                label="Search title or body"
                value={queryInput}
                onChange={(event) => setQueryInput(event.target.value)}
                placeholder="text"
              />
              <div className="issues-filter-actions">
                <Button type="submit">Apply filters</Button>
                <Button variant="quiet" onClick={clearFilters}>
                  Clear
                </Button>
              </div>
            </form>

            {issuesLoading ? (
              <LoadingState label="Loading synchronized issues…" />
            ) : issuesError ? (
              <ErrorState
                title="Issue snapshot unavailable"
                message={issuesError}
                action={
                  <Button onClick={() => setRefresh((value) => value + 1)}>
                    Retry
                  </Button>
                }
              />
            ) : !binding ? (
              <EmptyState
                title="No repository is bound"
                description="Bind a GitHub repository above, then run a sync to import its issues."
              />
            ) : issues && issues.items.length === 0 ? (
              <EmptyState
                title={
                  issues.synced_at === null
                    ? "No issues have been synchronized"
                    : labelFilter || appliedQuery || stateFilter !== "open"
                      ? "No issues match these filters"
                      : "GitHub reported no issues"
                }
                description={
                  issues.synced_at === null
                    ? "Run Sync issues to fetch the first bounded snapshot for this repository."
                    : stateFilter === "open" && !labelFilter && !appliedQuery
                      ? "The last successful sync found no issues in this repository."
                      : "Adjust the state, label or search filter."
                }
                action={
                  issues.synced_at === null && canSync ? (
                    <Button
                      variant="primary"
                      onClick={() => void onSync()}
                      disabled={busy !== null}
                    >
                      Sync issues
                    </Button>
                  ) : undefined
                }
              />
            ) : issues ? (
              <>
                <div className="issues-table-wrap">
                  <table className="issues-table">
                    <thead>
                      <tr>
                        {intakeEnabled && (
                          <th scope="col" className="issue-intake-check">
                            Select
                          </th>
                        )}
                        <th scope="col">#</th>
                        <th scope="col">Title</th>
                        <th scope="col">State</th>
                        <th scope="col">Labels</th>
                        <th scope="col">Author</th>
                        <th scope="col">Updated</th>
                      </tr>
                    </thead>
                    <tbody>
                      {issues.items.map((item) => (
                        <tr
                          key={item.number}
                          aria-current={
                            issueNumber === String(item.number)
                              ? "true"
                              : undefined
                          }
                        >
                          {intakeEnabled && (
                            <td className="issue-intake-check">
                              <input
                                type="checkbox"
                                aria-label={`Select issue #${item.number}`}
                                checked={
                                  selectedIssues[item.number] !== undefined
                                }
                                onChange={() => toggleIssueSelection(item)}
                              />
                            </td>
                          )}
                          <td className="mono">{item.number}</td>
                          <td>
                            <button
                              type="button"
                              className="issues-title"
                              onClick={() => openIssue(item.number)}
                            >
                              {item.title}
                            </button>
                          </td>
                          <td>{issueStateBadge(item.state)}</td>
                          <td>
                            {item.labels.length === 0 ? (
                              <span className="muted">—</span>
                            ) : (
                              <span className="issues-labels">
                                {item.labels.map((label) => (
                                  <span
                                    className="issues-label"
                                    key={`${item.number}-${label.name}`}
                                  >
                                    {label.name}
                                  </span>
                                ))}
                              </span>
                            )}
                          </td>
                          <td>{item.author ?? "—"}</td>
                          <td>{formatTimestamp(item.updated_at)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <div className="issues-pagination">
                  <span className="muted">
                    Showing {showingFrom}–{showingTo} of {total}
                  </span>
                  <Button
                    size="sm"
                    onClick={() => changeOffset(offset - PAGE_SIZE)}
                    disabled={offset === 0}
                  >
                    Previous
                  </Button>
                  <Button
                    size="sm"
                    onClick={() => changeOffset(offset + PAGE_SIZE)}
                    disabled={showingTo >= total}
                  >
                    Next
                  </Button>
                </div>
              </>
            ) : null}
          </>
        )}
      </Panel>

      {projectId !== "" && (
        <IssueIntakePanel
          projectId={projectId}
          binding={binding}
          connectionConfigured={connection?.configured === true}
          state={intake}
          loading={intakeLoading}
          loadError={intakeError}
          selected={selectedList}
          onRefresh={() => setRefresh((value) => value + 1)}
          onSelectionClear={() => setSelectedIssues({})}
        />
      )}

      {issueNumber !== "" && (
        <Panel className="issues-panel issues-detail">
          <div className="issues-panel-heading">
            <h2>Issue detail</h2>
            <Button variant="quiet" size="sm" onClick={closeIssue}>
              Close
            </Button>
          </div>
          {detailLoading ? (
            <LoadingState label="Loading issue…" />
          ) : detailError ? (
            <ErrorState
              title="Issue unavailable"
              message={detailError}
              action={<Button onClick={closeIssue}>Close</Button>}
            />
          ) : detail ? (
            <>
              <div className="issues-detail-title">
                <h3>
                  <span className="mono">#{detail.number}</span> {detail.title}
                </h3>
                {issueStateBadge(detail.state)}
              </div>
              <div className="issues-detail-meta">
                <span>Author: {detail.author ?? "unknown"}</span>
                <span>Updated {formatTimestamp(detail.updated_at)}</span>
                {detail.closed_at && (
                  <span>Closed {formatTimestamp(detail.closed_at)}</span>
                )}
                <span>
                  {detail.comments} comments on GitHub (not synchronized)
                </span>
              </div>
              {detail.labels.length > 0 && (
                <div className="issues-labels">
                  {detail.labels.map((label) => (
                    <span className="issues-label" key={label.name}>
                      {label.name}
                    </span>
                  ))}
                </div>
              )}
              {detail.body_truncated && (
                <p className="notice">
                  The stored body was truncated at the Core bound. Read the full
                  issue on GitHub.
                </p>
              )}
              <div className="issue-body">
                {detail.body || "No description provided."}
              </div>
              <p className="issues-note">
                Untrusted GitHub content shown as plain text. It is not an
                instruction to Factory agents and it grants no authority.
              </p>
              {isSafeHttpsUrl(detail.html_url) && (
                <a
                  className="issues-external"
                  href={detail.html_url}
                  target="_blank"
                  rel="noreferrer noopener"
                >
                  Open issue on GitHub
                </a>
              )}
            </>
          ) : null}
        </Panel>
      )}
    </div>
  );
}
