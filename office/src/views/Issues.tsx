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
import { shortId } from "../format";
import { useI18n } from "../i18n";
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
  const { t, formatDate, formatNumber } = useI18n();
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
          setConnectionError(message(cause, t("issues.error.loadState")));
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
  }, [t]);

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
          setIssuesError(message(cause, t("issues.error.snapshot")));
        }
      })
      .finally(() => {
        if (active) setIssuesLoading(false);
      });
    return () => {
      active = false;
    };
  }, [projectId, stateFilter, labelFilter, appliedQuery, offset, refresh, t]);

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
          setIntakeError(message(cause, t("issues.intake.error.load")));
        }
      })
      .finally(() => {
        if (active) setIntakeLoading(false);
      });
    return () => {
      active = false;
    };
  }, [projectId, refresh, t]);

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
      setDetailError(t("issues.error.issueNumber"));
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
              ? t("issues.error.issueGone")
              : message(cause, t("issues.error.detail")),
          );
        }
      })
      .finally(() => {
        if (active) setDetailLoading(false);
      });
    return () => {
      active = false;
    };
  }, [projectId, issueNumber, refresh, t]);

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
        t("issues.notice.connected", {
          login: next.connection.login ?? t("common.unknown"),
        }),
      );
      setRefresh((value) => value + 1);
    } catch (cause) {
      setActionError(message(cause, t("issues.error.connect")));
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
      setNotice(t("issues.notice.disconnected"));
      setRefresh((value) => value + 1);
    } catch (cause) {
      setActionError(message(cause, t("issues.error.disconnect")));
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
            ? t("issues.notice.rateLimited", {
                date: formatDate(next.next_allowed_at),
              })
            : next.detail,
        );
      } else if (next.repositories.length === 0) {
        setNotice(t("issues.notice.noRepositories"));
      }
    } catch (cause) {
      setActionError(message(cause, t("issues.error.repositories")));
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
        t("issues.notice.bound", {
          repo: result.binding.repo_full_name,
          branch: result.binding.default_branch,
        }),
      );
      setRefresh((value) => value + 1);
    } catch (cause) {
      setActionError(message(cause, t("issues.error.bind")));
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
          t("issues.notice.syncCompleted", {
            fetched: formatNumber(result.fetched),
            created: formatNumber(result.created),
            updated: formatNumber(result.updated),
            removed: formatNumber(result.removed),
          }),
        );
      } else {
        setActionError(
          result.detail ||
            t("issues.notice.syncStatus", { status: result.status }),
        );
      }
    } catch (cause) {
      setActionError(message(cause, t("issues.error.sync")));
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
        title={t("issues.title")}
        description={t("issues.description")}
        meta={
          <>
            <span>{t("issues.meta.snapshots")}</span>
            <span>{t("issues.meta.manual")}</span>
            <span>{t("issues.meta.noExecution")}</span>
          </>
        }
        actions={
          <>
            <Button
              variant="quiet"
              onClick={() => setRefresh((value) => value + 1)}
              disabled={busy !== null}
            >
              {t("issues.refresh")}
            </Button>
            <Button
              variant="primary"
              onClick={() => void onSync()}
              disabled={!canSync}
              title={
                backoffActive
                  ? t("issues.syncBackoffTitle", {
                      date: formatDate(nextAllowed ?? ""),
                    })
                  : binding === null
                    ? t("issues.syncBindFirst")
                    : connection?.configured
                      ? undefined
                      : t("issues.syncConnectFirst")
              }
            >
              {busy === "sync" ? t("issues.syncing") : t("issues.sync")}
            </Button>
          </>
        }
      />

      {connectionError && (
        <ErrorState
          title={t("issues.error.integrationUnavailable")}
          message={connectionError}
          action={
            <Button onClick={() => setRefresh((value) => value + 1)}>
              {t("common.retry")}
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
          <h2>{t("issues.connection.title")}</h2>
          {connection?.configured ? (
            <Badge tone="success">CONNECTED</Badge>
          ) : connection?.revoked_at ? (
            <Badge tone="danger">REVOKED</Badge>
          ) : (
            <Badge tone="neutral">NOT CONNECTED</Badge>
          )}
        </div>
        {connectionLoading ? (
          <LoadingState label={t("issues.connection.loading")} />
        ) : (
          <>
            <p className="muted">
              {connection?.detail ?? t("issues.connection.detailFallback")}
            </p>
            {connection?.login && (
              <dl className="issues-facts">
                <div>
                  <dt>{t("issues.connection.login")}</dt>
                  <dd>{connection.login}</dd>
                </div>
                <div>
                  <dt>{t("issues.connection.scopes")}</dt>
                  <dd>
                    {connection.scopes_reported
                      ? connection.scopes.length > 0
                        ? connection.scopes.join(", ")
                        : t("issues.connection.noneReported")
                      : t("issues.connection.scopesUnreported")}
                  </dd>
                </div>
                <div>
                  <dt>{t("issues.connection.verifiedAt")}</dt>
                  <dd>{formatDate(connection.verified_at ?? "")}</dd>
                </div>
                <div>
                  <dt>{t("issues.connection.fingerprint")}</dt>
                  <dd className="mono">
                    {connection.credential_fingerprint || "—"}
                  </dd>
                </div>
                <div>
                  <dt>{t("issues.connection.revision")}</dt>
                  <dd>{connection.revision}</dd>
                </div>
              </dl>
            )}
            <p className="issues-note">{t("issues.connection.note")}</p>
            {connection?.configured && (
              <details className="issues-details">
                <summary>{t("issues.connection.replace")}</summary>
                <form className="issues-inline-form" onSubmit={onConnect}>
                  <Input
                    label={t("issues.connection.token")}
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    value={token}
                    onChange={(event) => setToken(event.target.value)}
                    help={t("issues.connection.replaceHelp")}
                    required
                  />
                  <Button
                    type="submit"
                    variant="primary"
                    disabled={busy !== null || token.trim() === ""}
                  >
                    {busy === "connect"
                      ? t("issues.connection.verifying")
                      : t("issues.connection.verify")}
                  </Button>
                </form>
              </details>
            )}
            {!connection?.configured && (
              <form className="issues-inline-form" onSubmit={onConnect}>
                <Input
                  label={t("issues.connection.token")}
                  type="password"
                  autoComplete="off"
                  spellCheck={false}
                  value={token}
                  onChange={(event) => setToken(event.target.value)}
                  help={t("issues.connection.connectHelp")}
                  required
                />
                <Button
                  type="submit"
                  variant="primary"
                  disabled={busy !== null || token.trim() === ""}
                >
                  {busy === "connect"
                    ? t("issues.connection.verifying")
                    : t("issues.connection.verify")}
                </Button>
              </form>
            )}
            {connection?.login && (
              <div className="issues-actions">
                {confirmDisconnect ? (
                  <fieldset className="issues-confirm">
                    <legend className="issues-confirm-legend">
                      {t("issues.connection.disconnectLegend")}
                    </legend>
                    <p>{t("issues.connection.disconnectDescription")}</p>
                    <Button
                      variant="danger"
                      onClick={() => void onDisconnect()}
                      disabled={busy !== null}
                    >
                      {busy === "disconnect"
                        ? t("issues.connection.disconnecting")
                        : t("issues.connection.confirmDisconnect")}
                    </Button>
                    <Button
                      variant="quiet"
                      onClick={() => setConfirmDisconnect(false)}
                      disabled={busy !== null}
                    >
                      {t("issues.connection.cancel")}
                    </Button>
                  </fieldset>
                ) : (
                  <Button
                    variant="danger"
                    onClick={() => setConfirmDisconnect(true)}
                    disabled={busy !== null}
                  >
                    {t("issues.connection.disconnect")}
                  </Button>
                )}
              </div>
            )}
            {connectionState && connectionState.audit.length > 0 && (
              <details className="issues-details">
                <summary>
                  {t("issues.connection.audit", {
                    count: formatNumber(connectionState.audit.length),
                  })}
                </summary>
                <ul className="issues-audit">
                  {connectionState.audit.map((entry) => (
                    <li key={entry.audit_id}>
                      <strong>{entry.action}</strong>{" "}
                      {t("issues.connection.auditEntry", {
                        revision: entry.revision,
                        actor: entry.actor,
                        date: formatDate(entry.created_at),
                      })}
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
          <h2>{t("issues.project.title")}</h2>
          <Badge tone={binding ? "info" : "neutral"}>
            {binding ? "BOUND" : "UNBOUND"}
          </Badge>
        </div>
        {projectsLoading ? (
          <LoadingState label={t("issues.project.loading")} />
        ) : projects.length === 0 ? (
          <EmptyState
            title={t("issues.project.noneTitle")}
            description={t("issues.project.noneDescription")}
          />
        ) : (
          <>
            <Select
              label={t("issues.project.label")}
              value={projectId}
              onChange={(event) => selectProject(event.target.value)}
            >
              <option value="">{t("issues.project.select")}</option>
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.repository} ({shortId(project.id)})
                </option>
              ))}
            </Select>
            {binding ? (
              <dl className="issues-facts">
                <div>
                  <dt>{t("issues.repository.name")}</dt>
                  <dd className="mono">{binding.repo_full_name}</dd>
                </div>
                <div>
                  <dt>{t("issues.repository.branch")}</dt>
                  <dd className="mono">{binding.default_branch}</dd>
                </div>
                <div>
                  <dt>{t("issues.repository.boundAt")}</dt>
                  <dd>{formatDate(binding.bound_at)}</dd>
                </div>
              </dl>
            ) : (
              <p className="muted">{t("issues.repository.unbound")}</p>
            )}
            {binding && !bindingOpen && (
              <Button
                onClick={() => setBindingOpen(true)}
                disabled={busy !== null}
              >
                {t("issues.repository.change")}
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
                        : t("issues.repository.connectFirst")
                    }
                  >
                    {busy === "repositories"
                      ? t("issues.repository.browsing")
                      : t("issues.repository.browse")}
                  </Button>
                  {repositories?.rate_limited && (
                    <Badge tone="attention">RATE LIMITED</Badge>
                  )}
                </div>
                {repositories && repositories.repositories.length > 0 && (
                  <Select
                    label={t("issues.repository.visible")}
                    value={repositoryPick}
                    onChange={(event) => onPickRepository(event.target.value)}
                    help={t("issues.repository.visibleHelp")}
                  >
                    <option value="">
                      {t("issues.repository.visiblePlaceholder")}
                    </option>
                    {repositories.repositories.map((repository) => (
                      <option
                        key={repository.full_name}
                        value={repository.full_name}
                      >
                        {repository.full_name}
                        {repository.private
                          ? t("issues.repository.private")
                          : ""}
                      </option>
                    ))}
                  </Select>
                )}
                <div className="issues-bind-fields">
                  <Input
                    label={t("issues.repository.owner")}
                    value={bindingOwner}
                    onChange={(event) => setBindingOwner(event.target.value)}
                    placeholder={t("issues.repository.ownerPlaceholder")}
                    required
                  />
                  <Input
                    label={t("issues.repository.name")}
                    value={bindingRepo}
                    onChange={(event) => setBindingRepo(event.target.value)}
                    placeholder={t("issues.repository.repoPlaceholder")}
                    required
                  />
                  <Input
                    label={t("issues.repository.branch")}
                    value={bindingBranch}
                    onChange={(event) => setBindingBranch(event.target.value)}
                    placeholder={t("issues.repository.branchPlaceholder")}
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
                    {busy === "bind"
                      ? t("issues.repository.binding")
                      : t("issues.repository.bind")}
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
                      {t("issues.connection.cancel")}
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
          <h2>{t("issues.list.title")}</h2>
          <Badge tone={statusBadge.tone}>{statusBadge.label}</Badge>
        </div>
        <div className="issues-sync-state">
          <span>
            {issues?.synced_at
              ? t("issues.list.lastSync", {
                  date: formatDate(issues.synced_at),
                })
              : t("issues.list.neverSynced")}
          </span>
          {issues?.stale && <Badge tone="attention">STALE</Badge>}
          {issues?.truncated && (
            <Badge tone="attention">BOUNDED SNAPSHOT</Badge>
          )}
          {nextAllowed && (
            <span>
              {t("issues.list.retryAfter", {
                date: formatDate(nextAllowed),
              })}
            </span>
          )}
          {backoffActive && <Badge tone="attention">BACKOFF ACTIVE</Badge>}
          {lastSync && (
            <span>
              {t("issues.list.lastRun", {
                fetched: formatNumber(lastSync.fetched),
                created: formatNumber(lastSync.created),
                updated: formatNumber(lastSync.updated),
                removed: formatNumber(lastSync.removed),
              })}
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
            {t("issues.list.backoffNotice", {
              date: formatDate(nextAllowed),
            })}
          </p>
        )}
        {issues?.truncated && (
          <p className="issues-note">{t("issues.list.truncatedNotice")}</p>
        )}

        {projectId === "" ? (
          <EmptyState
            title={t("issues.list.selectProjectTitle")}
            description={t("issues.list.selectProjectDescription")}
          />
        ) : (
          <>
            <form className="issues-filters" onSubmit={applyFilters}>
              <Select
                label={t("issues.filters.state")}
                value={stateFilter}
                onChange={(event) =>
                  selectState(event.target.value as IssueStateFilter)
                }
              >
                <option value="open">{t("issues.filters.open")}</option>
                <option value="closed">{t("issues.filters.closed")}</option>
                <option value="all">{t("issues.filters.allStates")}</option>
              </Select>
              <Input
                label={t("issues.filters.label")}
                value={labelInput}
                onChange={(event) => setLabelInput(event.target.value)}
                placeholder={t("issues.filters.labelPlaceholder")}
              />
              <Input
                label={t("issues.filters.search")}
                value={queryInput}
                onChange={(event) => setQueryInput(event.target.value)}
                placeholder={t("issues.filters.searchPlaceholder")}
              />
              <div className="issues-filter-actions">
                <Button type="submit">{t("issues.filters.apply")}</Button>
                <Button variant="quiet" onClick={clearFilters}>
                  {t("issues.filters.clear")}
                </Button>
              </div>
            </form>

            {issuesLoading ? (
              <LoadingState label={t("issues.list.loading")} />
            ) : issuesError ? (
              <ErrorState
                title={t("issues.list.unavailableTitle")}
                message={issuesError}
                action={
                  <Button onClick={() => setRefresh((value) => value + 1)}>
                    {t("common.retry")}
                  </Button>
                }
              />
            ) : !binding ? (
              <EmptyState
                title={t("issues.list.unboundTitle")}
                description={t("issues.list.unboundDescription")}
              />
            ) : issues && issues.items.length === 0 ? (
              <EmptyState
                title={
                  issues.synced_at === null
                    ? t("issues.list.emptyNeverTitle")
                    : labelFilter || appliedQuery || stateFilter !== "open"
                      ? t("issues.list.emptyFilteredTitle")
                      : t("issues.list.emptyRepoTitle")
                }
                description={
                  issues.synced_at === null
                    ? t("issues.list.emptyNeverDescription")
                    : stateFilter === "open" && !labelFilter && !appliedQuery
                      ? t("issues.list.emptyRepoDescription")
                      : t("issues.list.emptyFilteredDescription")
                }
                action={
                  issues.synced_at === null && canSync ? (
                    <Button
                      variant="primary"
                      onClick={() => void onSync()}
                      disabled={busy !== null}
                    >
                      {t("issues.sync")}
                    </Button>
                  ) : undefined
                }
              />
            ) : issues ? (
              <>
                <div className="issues-table-wrap table-responsive">
                  <table
                    className="issues-table"
                    aria-label={t("issues.table.label")}
                  >
                    <thead>
                      <tr>
                        {intakeEnabled && (
                          <th scope="col" className="issue-intake-check">
                            {t("issues.intake.select")}
                          </th>
                        )}
                        <th scope="col">{t("issues.table.number")}</th>
                        <th scope="col">{t("issues.table.title")}</th>
                        <th scope="col">{t("issues.table.state")}</th>
                        <th scope="col">{t("issues.table.labels")}</th>
                        <th scope="col">{t("issues.table.author")}</th>
                        <th scope="col">{t("issues.table.updated")}</th>
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
                                aria-label={t("issues.intake.selectIssue", {
                                  number: item.number,
                                })}
                                checked={
                                  selectedIssues[item.number] !== undefined
                                }
                                onChange={() => toggleIssueSelection(item)}
                              />
                            </td>
                          )}
                          <td
                            className="mono"
                            data-label={t("issues.table.number")}
                          >
                            {item.number}
                          </td>
                          <td data-label={t("issues.table.title")}>
                            <button
                              type="button"
                              className="issues-title"
                              onClick={() => openIssue(item.number)}
                            >
                              {item.title}
                            </button>
                          </td>
                          <td data-label={t("issues.table.state")}>
                            {issueStateBadge(item.state)}
                          </td>
                          <td data-label={t("issues.table.labels")}>
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
                          <td data-label={t("issues.table.author")}>
                            {item.author ?? "—"}
                          </td>
                          <td data-label={t("issues.table.updated")}>
                            {formatDate(item.updated_at)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <div className="issues-pagination">
                  <span className="muted">
                    {t("issues.pagination.showing", {
                      from: formatNumber(showingFrom),
                      to: formatNumber(showingTo),
                      total: formatNumber(total),
                    })}
                  </span>
                  <Button
                    size="sm"
                    onClick={() => changeOffset(offset - PAGE_SIZE)}
                    disabled={offset === 0}
                  >
                    {t("issues.pagination.previous")}
                  </Button>
                  <Button
                    size="sm"
                    onClick={() => changeOffset(offset + PAGE_SIZE)}
                    disabled={showingTo >= total}
                  >
                    {t("issues.pagination.next")}
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
            <h2>{t("issues.detail.title")}</h2>
            <Button variant="quiet" size="sm" onClick={closeIssue}>
              {t("issues.detail.close")}
            </Button>
          </div>
          {detailLoading ? (
            <LoadingState label={t("issues.detail.loading")} />
          ) : detailError ? (
            <ErrorState
              title={t("issues.detail.unavailableTitle")}
              message={detailError}
              action={
                <Button onClick={closeIssue}>{t("issues.detail.close")}</Button>
              }
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
                <span>
                  {t("issues.detail.author", {
                    author: detail.author ?? t("common.unknown"),
                  })}
                </span>
                <span>
                  {t("issues.detail.updated", {
                    date: formatDate(detail.updated_at),
                  })}
                </span>
                {detail.closed_at && (
                  <span>
                    {t("issues.detail.closed", {
                      date: formatDate(detail.closed_at),
                    })}
                  </span>
                )}
                <span>
                  {t("issues.detail.comments", {
                    count: formatNumber(detail.comments),
                  })}
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
                <p className="notice">{t("issues.detail.truncated")}</p>
              )}
              <div className="issue-body">
                {detail.body || t("issues.detail.noDescription")}
              </div>
              <p className="issues-note">{t("issues.detail.untrusted")}</p>
              {isSafeHttpsUrl(detail.html_url) && (
                <a
                  className="issues-external"
                  href={detail.html_url}
                  target="_blank"
                  rel="noreferrer noopener"
                >
                  {t("issues.detail.open")}
                </a>
              )}
            </>
          ) : null}
        </Panel>
      )}
    </div>
  );
}
