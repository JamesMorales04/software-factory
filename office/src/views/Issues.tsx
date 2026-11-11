import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import { Badge, type BadgeTone } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Checkbox, Input, Select, Textarea } from "../components/ui/Field";
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
  type GitHubIssueSummary,
  type GitHubRepositoryList,
  type GitHubSyncState,
  type IntakeRequestSummary,
  type IssueIntakeQueueItem,
  type IssueIntakeState,
  newIdempotencyKey,
  type Project,
  type ProjectConfig,
  type ProjectGitHubBinding,
  type ResolvedExecutionProfile,
} from "../coreApi";
import { formatTimestamp } from "../format";
import { type MessageKey, useI18n } from "../i18n";
import { projectDisplayName } from "../workspace";
import "./issues.css";

const COLUMNS = [
  { id: "ready", labelKey: "issues.board.ready", tone: "success" },
  { id: "queued", labelKey: "issues.board.queued", tone: "info" },
  { id: "working", labelKey: "issues.board.working", tone: "accent" },
  { id: "review", labelKey: "issues.board.review", tone: "info" },
  { id: "attention", labelKey: "issues.board.attention", tone: "attention" },
  { id: "done", labelKey: "issues.board.done", tone: "neutral" },
] as const;

type ColumnId = (typeof COLUMNS)[number]["id"];

interface BoardProject {
  project: Project;
  binding: ProjectGitHubBinding | null;
  sync: GitHubSyncState | null;
  issues: GitHubIssueSummary[];
  intake: IssueIntakeState | null;
  total: number;
  truncated: boolean;
  error: string | null;
}

interface CardData {
  project: Project;
  issue: GitHubIssueSummary;
  state: ColumnId;
  queue: IssueIntakeQueueItem | null;
  intakeRequestId: string | null;
  reason: string | null;
}

function errorText(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function cardState(
  board: BoardProject,
  issue: GitHubIssueSummary,
  requestById: Map<string, IntakeRequestSummary>,
  t: (key: MessageKey) => string,
): Omit<CardData, "project" | "issue"> {
  const link = issue.issueLink;
  if (link) {
    const reason =
      link.latestErrorDetail !== null
        ? `${link.latestErrorKind ?? "GITHUB_WRITE_FAILED"}: ${link.latestErrorDetail}`
        : null;
    if (
      link.deliveryState === "FAILED" ||
      link.deliveryState === "BLOCKED" ||
      link.deliveryState === "PARTIAL"
    ) {
      return {
        state: "attention",
        queue: null,
        intakeRequestId: link.requestId,
        reason,
      };
    }
    switch (link.state) {
      case "DONE":
        return {
          state: "done",
          queue: null,
          intakeRequestId: link.requestId,
          reason: null,
        };
      case "IN_REVIEW":
        return {
          state: "review",
          queue: null,
          intakeRequestId: link.requestId,
          reason: null,
        };
      case "NEEDS_ATTENTION":
        return {
          state: "attention",
          queue: null,
          intakeRequestId: link.requestId,
          reason,
        };
      case "IN_PROGRESS":
        return {
          state: "working",
          queue: null,
          intakeRequestId: link.requestId,
          reason: null,
        };
      case "QUEUED":
        return {
          state: "queued",
          queue: null,
          intakeRequestId: link.requestId,
          reason: null,
        };
      case "READY":
        return {
          state: "ready",
          queue: null,
          intakeRequestId: link.requestId,
          reason: null,
        };
    }
  }
  const queue = (board.intake?.queue ?? [])
    .filter((item) => item.issue_number === issue.number)
    .sort((left, right) => right.updated_at.localeCompare(left.updated_at))[0];
  const batchItem = (board.intake?.batches ?? [])
    .flatMap((batch) => batch.items)
    .find((item) => item.item.issue_number === issue.number);
  if (queue) {
    if (queue.status === "PENDING" || queue.status === "LEASED") {
      return { state: "queued", queue, intakeRequestId: null, reason: null };
    }
    if (queue.status === "RECONCILED") {
      const intakeRequestId = batchItem?.item.intake_request_id ?? null;
      const request = intakeRequestId
        ? requestById.get(intakeRequestId)
        : undefined;
      if (request) {
        if (request.state === "COMPLETED") {
          return { state: "done", queue, intakeRequestId, reason: null };
        }
        if (request.state === "CANCELLED") {
          return {
            state: "attention",
            queue,
            intakeRequestId,
            reason: t("issues.board.requestCancelled"),
          };
        }
        if (request.state === "FAILED" || request.state === "BLOCKED") {
          return {
            state: "attention",
            queue,
            intakeRequestId,
            reason: t(
              request.state === "FAILED"
                ? "issues.board.requestFailed"
                : "issues.board.requestBlocked",
            ),
          };
        }
      }
      return {
        state: request ? "working" : "queued",
        queue,
        intakeRequestId,
        reason: request ? null : t("issues.board.requestUnknown"),
      };
    }
    return {
      state: "attention",
      queue,
      intakeRequestId: batchItem?.item.intake_request_id ?? null,
      reason: queue.reasons[0] ?? queue.reason,
    };
  }
  if (issue.state === "closed") {
    return { state: "done", queue: null, intakeRequestId: null, reason: null };
  }
  if (!board.intake?.policy?.enabled) {
    return {
      state: "attention",
      queue: null,
      intakeRequestId: null,
      reason: t("issues.board.policyDisabled"),
    };
  }
  return { state: "ready", queue: null, intakeRequestId: null, reason: null };
}

function toneFor(state: ColumnId): BadgeTone {
  return COLUMNS.find((column) => column.id === state)?.tone ?? "neutral";
}

export function Issues() {
  const { t } = useI18n();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const projectId = searchParams.get("project") ?? "";
  const [projects, setProjects] = useState<Project[]>([]);
  const [connection, setConnection] = useState<GitHubConnectionState | null>(
    null,
  );
  const [boards, setBoards] = useState<BoardProject[] | null>(null);
  const [requests, setRequests] = useState<IntakeRequestSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [autoStart, setAutoStart] = useState(
    () => localStorage.getItem("factory-issues-auto-start") === "1",
  );
  const autoStarting = useRef(false);
  const autoAttempted = useRef(new Set<string>());
  const [busy, setBusy] = useState<string | null>(null);
  const [selected, setSelected] = useState<CardData | null>(null);
  const [setupOpen, setSetupOpen] = useState(false);
  const [token, setToken] = useState("");
  const [repositories, setRepositories] = useState<GitHubRepositoryList | null>(
    null,
  );
  const [bindProject, setBindProject] = useState("");
  const [bindRepository, setBindRepository] = useState("");
  const [policyProject, setPolicyProject] = useState<BoardProject | null>(null);
  const [policyLabels, setPolicyLabels] = useState("factory:r0");
  const [policyMaxIssues, setPolicyMaxIssues] = useState("3");
  const [policyExpiry, setPolicyExpiry] = useState(
    new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString().slice(0, 16),
  );
  const [policyReason, setPolicyReason] = useState("");
  const [policyAcknowledged, setPolicyAcknowledged] = useState(false);
  const [policyConfig, setPolicyConfig] = useState<ProjectConfig | null>(null);
  const [policyProfile, setPolicyProfile] =
    useState<ResolvedExecutionProfile | null>(null);
  const autoSync = useRef(new Set<string>());
  const autoSyncBusy = useRef(false);

  const load = useCallback(async () => {
    try {
      const [nextProjects, nextConnection, nextRequests] = await Promise.all([
        coreApi.listProjects(),
        coreApi.getGitHubConnection(),
        coreApi.listIntakeRequests({ limit: 100 }),
      ]);
      const targets = projectId
        ? nextProjects.filter((project) => project.id === projectId)
        : nextProjects;
      const nextBoards = await Promise.all(
        targets.map(async (project) => {
          try {
            const [issueList, intake, github] = await Promise.all([
              coreApi.listProjectIssues(project.id, {
                state: "all",
                limit: 100,
              }),
              coreApi.getIssueIntake(project.id).catch(() => null),
              coreApi.getProjectGitHub(project.id).catch(() => null),
            ]);
            return {
              project,
              binding: issueList.binding ?? github?.binding ?? null,
              sync: github?.sync_state ?? null,
              issues: issueList.items,
              intake,
              total: issueList.total,
              truncated: issueList.truncated,
              error: null,
            } satisfies BoardProject;
          } catch (cause) {
            return {
              project,
              binding: null,
              sync: null,
              issues: [],
              intake: null,
              total: 0,
              truncated: false,
              error: errorText(cause, t("issues.error.loadState")),
            } satisfies BoardProject;
          }
        }),
      );
      const linkedRequestIds = new Set(
        nextBoards.flatMap((board) =>
          (board.intake?.batches ?? []).flatMap((batch) =>
            batch.items
              .map((item) => item.item.intake_request_id)
              .filter((id): id is string => id !== null),
          ),
        ),
      );
      const known = new Set(nextRequests.map((request) => request.id));
      const missing = [...linkedRequestIds]
        .filter((id) => !known.has(id))
        .slice(0, 50);
      if (missing.length > 0) {
        const extra = await Promise.all(
          missing.map((id) => coreApi.getIntakeRequest(id).catch(() => null)),
        );
        for (const request of extra) {
          if (request) nextRequests.push(request);
        }
      }
      setProjects(nextProjects);
      setConnection(nextConnection);
      setRequests(nextRequests);
      setBoards(nextBoards);
      setError(null);
    } catch (cause) {
      setError(errorText(cause, t("issues.error.loadState")));
    }
  }, [projectId, t]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), 10_000);
    return () => window.clearInterval(timer);
  }, [load]);

  useEffect(() => {
    if (!connection?.connection.configured || !boards || autoSyncBusy.current) {
      return;
    }
    const stale = boards.find(
      (board) =>
        board.binding &&
        !autoSync.current.has(board.project.id) &&
        (board.sync === null ||
          board.sync.status === "NEVER_SYNCED" ||
          board.sync.stale),
    );
    if (!stale) return;
    autoSync.current.add(stale.project.id);
    autoSyncBusy.current = true;
    void coreApi
      .syncProjectIssues(stale.project.id)
      .then(() => load())
      .catch(() => undefined)
      .finally(() => {
        autoSyncBusy.current = false;
      });
  }, [boards, connection, load]);

  const requestById = useMemo(
    () => new Map(requests.map((request) => [request.id, request])),
    [requests],
  );

  const counts = useMemo(() => {
    const result: Record<ColumnId, number> = {
      ready: 0,
      queued: 0,
      working: 0,
      review: 0,
      attention: 0,
      done: 0,
    };
    for (const board of boards ?? []) {
      for (const issue of board.issues) {
        result[cardState(board, issue, requestById, t).state] += 1;
      }
    }
    return result;
  }, [boards, requestById, t]);

  const cards = useMemo(
    () =>
      (boards ?? []).flatMap((board) =>
        board.issues.map(
          (issue): CardData => ({
            project: board.project,
            issue,
            ...cardState(board, issue, requestById, t),
          }),
        ),
      ),
    [boards, requestById, t],
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: auto-start uses the latest rendered handler and card projection.
  useEffect(() => {
    localStorage.setItem("factory-issues-auto-start", autoStart ? "1" : "0");
    if (!autoStart || autoStarting.current || busy !== null) return;
    const ready = cards.find(
      (card) =>
        card.state === "ready" &&
        !card.issue.issueLink &&
        !autoAttempted.current.has(`${card.project.id}:${card.issue.number}`),
    );
    if (!ready) return;
    autoStarting.current = true;
    autoAttempted.current.add(`${ready.project.id}:${ready.issue.number}`);
    const timer = window.setTimeout(() => {
      void startIssue(ready, false).finally(() => {
        autoStarting.current = false;
      });
    }, 1200);
    return () => window.clearTimeout(timer);
  }, [autoStart, busy, cards]);

  function setProject(value: string) {
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (value) next.set("project", value);
        else next.delete("project");
        return next;
      },
      { replace: true },
    );
  }

  async function refreshIssues() {
    const bound = (boards ?? []).filter((board) => board.binding !== null);
    if (bound.length === 0) return;
    setBusy("sync");
    setError(null);
    setNotice(null);
    try {
      for (const board of bound) {
        await coreApi.syncProjectIssues(board.project.id);
      }
      autoSync.current.clear();
      await load();
      setNotice(t("issues.refresh.done"));
    } catch (cause) {
      setError(errorText(cause, t("issues.error.sync")));
    } finally {
      setBusy(null);
    }
  }

  async function reopenBinding() {
    if (!selected) return;
    setBusy("binding");
    setSetupOpen(true);
    setBindProject(selected.project.id);
    try {
      const listing = await coreApi.listGitHubRepositories();
      setRepositories(listing);
      const binding = boards?.find(
        (board) => board.project.id === selected.project.id,
      )?.binding;
      setBindRepository(binding?.repo_full_name ?? "");
    } catch (cause) {
      setError(errorText(cause, t("issues.error.repositories")));
    } finally {
      setBusy(null);
    }
  }

  async function retryDelivery(card: CardData) {
    const link = card.issue.issueLink;
    if (!link?.latestErrorEventId) return;
    const key = `${link.id}:${link.latestErrorEventId}`;
    setBusy(`retry:${key}`);
    setError(null);
    setNotice(null);
    try {
      await coreApi.retryIssueDelivery(link.id, link.latestErrorEventId);
      setNotice(t("issues.delivery.retried", { number: card.issue.number }));
      await load();
    } catch (cause) {
      setError(errorText(cause, t("issues.delivery.retryError")));
    } finally {
      setBusy(null);
    }
  }

  async function startIssue(card: CardData, navigateToRequest = true) {
    const link = card.issue.issueLink;
    if (
      link &&
      !(link.state === "NEEDS_ATTENTION" && link.latestErrorEventId === null)
    ) {
      if (navigateToRequest) {
        navigate(`/requests/${link.requestId}`);
      }
      return;
    }
    const board = boards?.find((entry) => entry.project.id === card.project.id);
    if (!board?.intake?.policy?.enabled) {
      setPolicyProject(board ?? null);
      setSelected(null);
      return;
    }
    if (!board.binding?.repository_github_id) {
      setSelected(card);
      await reopenBinding();
      return;
    }
    setBusy(`start:${card.project.id}:${card.issue.number}`);
    setError(null);
    try {
      const policy = board.intake.policy;
      const batch = await coreApi.createIssueIntakeBatch(card.project.id, {
        policy_id: policy.id,
        expected_policy_revision: policy.revision,
        expected_policy_hash: policy.content_hash,
        items: [
          {
            number: card.issue.number,
            github_issue_id: card.issue.github_id,
            content_hash: card.issue.content_hash,
            updated_at: card.issue.updated_at,
          },
        ],
        expires_at: new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString(),
        acknowledge: true,
        reason: t("issues.start.reason", { number: card.issue.number }),
        idempotency_key: newIdempotencyKey(),
      });
      const result = await coreApi.dispatchIssueIntake(
        card.project.id,
        batch.batch.id,
      );
      if (result.intake?.id) {
        if (navigateToRequest) {
          navigate(`/requests/${result.intake.id}`);
        } else {
          setNotice(t("issues.start.started", { number: card.issue.number }));
          await load();
        }
        return;
      }
      setError(
        result.reasons.length > 0
          ? t("issues.start.blocked", { reasons: result.reasons.join(", ") })
          : t("issues.start.pending"),
      );
      await load();
    } catch (cause) {
      setError(errorText(cause, t("issues.start.error")));
    } finally {
      setBusy(null);
    }
  }

  async function openPolicy(board: BoardProject) {
    setPolicyProject(board);
    setPolicyLabels(
      board.intake?.policy?.required_labels.join(", ") ?? "factory:r0",
    );
    setPolicyMaxIssues(String(board.intake?.policy?.max_issues ?? 3));
    setPolicyReason("");
    setPolicyAcknowledged(false);
    setError(null);
    try {
      const [config, profile] = await Promise.all([
        coreApi.getProjectConfig(board.project.id),
        coreApi.getProjectExecutionProfile(board.project.id),
      ]);
      setPolicyConfig(config);
      setPolicyProfile(profile);
    } catch (cause) {
      setError(errorText(cause, t("issues.intake.error.configUnavailable")));
    }
  }

  async function savePolicy(event: React.FormEvent) {
    event.preventDefault();
    if (!policyProject || !policyConfig || !policyProfile) return;
    if (!policyProject.binding?.repository_github_id) {
      setError(t("issues.intake.error.repositoryIdentity"));
      return;
    }
    if (!policyAcknowledged) {
      setError(t("issues.intake.error.acknowledgePolicy"));
      return;
    }
    setBusy("policy");
    setError(null);
    try {
      await coreApi.configureIssueIntakePolicy(policyProject.project.id, {
        repository_full_name: policyProject.binding.repo_full_name,
        repository_github_id: policyProject.binding.repository_github_id,
        required_labels: policyLabels
          .split(",")
          .map((value) => value.trim())
          .filter(Boolean),
        excluded_labels: [],
        author_ids: [],
        author_associations: ["MEMBER"],
        execution_profile_id: policyProfile.profileId,
        profile_revision_id: policyProfile.revisionId,
        profile_revision: policyProfile.revision,
        profile_hash: policyProfile.contentHash,
        project_config_hash: policyConfig.configHash,
        allowed_paths: policyConfig.allowedPaths,
        capabilities: policyConfig.capabilities,
        max_issues: Number.parseInt(policyMaxIssues, 10) || 3,
        max_concurrent: 1,
        expires_at: new Date(policyExpiry).toISOString(),
        acknowledge: true,
        reason: policyReason.trim() || t("issues.intake.defaultReason"),
        idempotency_key: newIdempotencyKey(),
      });
      setPolicyProject(null);
      await load();
    } catch (cause) {
      setError(errorText(cause, t("issues.intake.error.savePolicy")));
    } finally {
      setBusy(null);
    }
  }

  async function saveToken(event: React.FormEvent) {
    event.preventDefault();
    if (!token.trim()) return;
    setBusy("token");
    setError(null);
    try {
      const next = await coreApi.updateGitHubConnection(token.trim());
      setConnection(next);
      setToken("");
      setRepositories(await coreApi.listGitHubRepositories());
      setSetupOpen(true);
    } catch (cause) {
      setError(errorText(cause, t("issues.error.connect")));
    } finally {
      setBusy(null);
    }
  }

  async function loadRepositories() {
    setBusy("repositories");
    setError(null);
    try {
      setRepositories(await coreApi.listGitHubRepositories());
    } catch (cause) {
      setError(errorText(cause, t("issues.error.repositories")));
    } finally {
      setBusy(null);
    }
  }

  async function bindSelectedRepository(event: React.FormEvent) {
    event.preventDefault();
    if (!bindProject || !bindRepository) return;
    const repository = repositories?.repositories.find(
      (item) => item.full_name === bindRepository,
    );
    if (!repository) return;
    const [owner, repo] = repository.full_name.split("/");
    setBusy("bind");
    setError(null);
    try {
      await coreApi.bindProjectGitHub(bindProject, {
        owner,
        repo,
        repository_github_id: repository.id ?? null,
      });
      setSetupOpen(false);
      await load();
    } catch (cause) {
      setError(errorText(cause, t("issues.error.bind")));
    } finally {
      setBusy(null);
    }
  }

  const github = connection?.connection ?? null;

  return (
    <div className="issues-workspace">
      <PageHeader
        title={t("issues.title")}
        description={t("issues.descriptionSimplified")}
        actions={
          <div className="row">
            <Button
              variant="secondary"
              onClick={() => void refreshIssues()}
              disabled={
                busy === "sync" ||
                !(boards ?? []).some((board) => board.binding !== null)
              }
            >
              {busy === "sync"
                ? t("issues.refreshing")
                : t("issues.refreshAction")}
            </Button>
            <Button
              variant="secondary"
              onClick={() => setSetupOpen((open) => !open)}
            >
              {t("issues.github.setup")}
            </Button>
            <Link className="ui-button ui-button--primary ui-button--md" to="/">
              {t("issues.newRequest")}
            </Link>
          </div>
        }
      />

      <Panel className="issues-toolbar">
        <Select
          label={t("issues.filter.project")}
          value={projectId}
          onChange={(event) => setProject(event.target.value)}
        >
          <option value="">{t("issues.filter.allProjects")}</option>
          {projects.map((project) => (
            <option key={project.id} value={project.id}>
              {projectDisplayName(project)}
            </option>
          ))}
        </Select>
        <label className="issues-auto-start">
          <input
            type="checkbox"
            checked={autoStart}
            onChange={(event) => setAutoStart(event.target.checked)}
          />
          <span>{t("issues.autoStart.label")}</span>
          <small className="muted">{t("issues.autoStart.help")}</small>
        </label>
        <div className="issues-board-counts">
          {COLUMNS.map((column) => (
            <Badge key={column.id} tone={toneFor(column.id)}>
              {t(column.labelKey)}: {counts[column.id]}
            </Badge>
          ))}
        </div>
      </Panel>

      {setupOpen && (
        <Panel className="issues-setup">
          <div className="section-heading">
            <h2>{t("issues.github.title")}</h2>
            <Badge tone={github?.configured ? "success" : "attention"}>
              {github?.configured
                ? t("issues.github.connected")
                : t("issues.github.disconnected")}
            </Badge>
          </div>
          {!github?.configured ? (
            <form className="issues-setup-form" onSubmit={saveToken}>
              <Input
                label={t("issues.github.token")}
                type="password"
                value={token}
                onChange={(event) => setToken(event.target.value)}
                autoComplete="new-password"
                help={t("issues.github.tokenHelp")}
              />
              <Button
                type="submit"
                variant="primary"
                disabled={busy === "token"}
              >
                {busy === "token"
                  ? t("issues.github.connecting")
                  : t("issues.github.connect")}
              </Button>
            </form>
          ) : (
            <div className="issues-setup-form">
              <p className="muted">
                {t("issues.github.connectedAs", {
                  login: github.login ?? "GitHub",
                })}
              </p>
              <div className="row">
                <Button
                  variant="secondary"
                  onClick={() => void loadRepositories()}
                  disabled={busy === "repositories"}
                >
                  {t("issues.github.loadRepositories")}
                </Button>
              </div>
              <form
                className="issues-setup-grid"
                onSubmit={bindSelectedRepository}
              >
                <Select
                  label={t("issues.github.project")}
                  value={bindProject}
                  onChange={(event) => setBindProject(event.target.value)}
                  required
                >
                  <option value="">{t("issues.github.selectProject")}</option>
                  {projects.map((project) => (
                    <option key={project.id} value={project.id}>
                      {projectDisplayName(project)}
                    </option>
                  ))}
                </Select>
                <Select
                  label={t("issues.github.repository")}
                  value={bindRepository}
                  onChange={(event) => setBindRepository(event.target.value)}
                  required
                  disabled={repositories === null}
                >
                  <option value="">
                    {repositories === null
                      ? t("issues.github.loadingRepositories")
                      : t("issues.github.selectRepository")}
                  </option>
                  {(repositories?.repositories ?? []).map((repository) => (
                    <option
                      key={repository.full_name}
                      value={repository.full_name}
                    >
                      {repository.full_name}
                    </option>
                  ))}
                </Select>
                <Button
                  type="submit"
                  variant="primary"
                  disabled={busy === "bind"}
                >
                  {busy === "bind"
                    ? t("issues.github.binding")
                    : t("issues.github.bind")}
                </Button>
              </form>
            </div>
          )}
        </Panel>
      )}

      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}
      {boards?.some((board) => board.error) && (
        <div className="ui-callout ui-callout--attention" role="alert">
          <strong>{t("issues.board.partialTitle")}</strong>
          <p>
            {boards
              .filter((board) => board.error)
              .map((board) => projectDisplayName(board.project))
              .join(", ")}
          </p>
          <p className="muted">{t("issues.board.partialHelp")}</p>
          <Button variant="secondary" onClick={() => void load()}>
            {t("common.retry")}
          </Button>
        </div>
      )}
      {boards?.some((board) => board.truncated) && (
        <p className="ui-callout ui-callout--attention">
          {t("issues.board.truncated", {
            count: boards
              .filter((board) => board.truncated)
              .reduce((sum, board) => sum + board.total, 0),
          })}
        </p>
      )}

      {error && (
        <ErrorState
          message={error}
          action={
            <Button onClick={() => void load()}>{t("common.retry")}</Button>
          }
        />
      )}
      {!error && boards === null && (
        <LoadingState label={t("issues.loading")} />
      )}
      {!error && boards !== null && boards.length === 0 && (
        <Panel>
          <EmptyState
            title={t("issues.emptyTitle")}
            description={t("issues.emptyDescription")}
            action={
              <Link
                className="ui-button ui-button--primary ui-button--md"
                to="/projects"
              >
                {t("issues.openProjects")}
              </Link>
            }
          />
        </Panel>
      )}

      {!error && boards !== null && boards.length > 0 && (
        <section className="issue-board" aria-label={t("issues.board.label")}>
          {COLUMNS.map((column) => {
            const columnCards = cards.filter(
              (card) => card.state === column.id,
            );
            return (
              <section
                className="issue-column"
                key={column.id}
                aria-labelledby={`issues-${column.id}`}
              >
                <header className="issue-column-head">
                  <h2 id={`issues-${column.id}`}>{t(column.labelKey)}</h2>
                  <Badge tone={column.tone}>{columnCards.length}</Badge>
                </header>
                {columnCards.length === 0 && (
                  <p className="muted issue-column-empty">
                    {t("issues.board.empty")}
                  </p>
                )}
                {columnCards.map((card) => (
                  <button
                    type="button"
                    className="issue-card"
                    key={`${card.project.id}:${card.issue.number}`}
                    onClick={() => setSelected(card)}
                  >
                    <span className="issue-card-project">
                      {projectDisplayName(card.project)}
                    </span>
                    <span className="issue-card-title">
                      #{card.issue.number} {card.issue.title}
                    </span>
                    <span className="issue-card-meta">
                      {card.issue.labels.slice(0, 3).map((label) => (
                        <span className="issue-label" key={label.name}>
                          {label.name}
                        </span>
                      ))}
                      <span>{formatTimestamp(card.issue.updated_at)}</span>
                    </span>
                    {card.reason && (
                      <span className="issue-card-reason">{card.reason}</span>
                    )}
                  </button>
                ))}
              </section>
            );
          })}
        </section>
      )}

      {selected && (
        <div className="issue-detail-scrim">
          <button
            type="button"
            className="issue-detail-backdrop"
            aria-label={t("issues.detail.close")}
            onClick={() => setSelected(null)}
          />
          <aside
            className="issue-detail"
            role="dialog"
            aria-modal="true"
            aria-labelledby="issue-detail-title"
          >
            <header className="issue-detail-head">
              <div>
                <p className="muted">{projectDisplayName(selected.project)}</p>
                <h2 id="issue-detail-title">
                  #{selected.issue.number} {selected.issue.title}
                </h2>
              </div>
              <Button variant="quiet" onClick={() => setSelected(null)}>
                {t("issues.detail.close")}
              </Button>
            </header>
            <div className="issue-detail-meta">
              <Badge tone={toneFor(selected.state)}>
                {t(`issues.board.${selected.state}`)}
              </Badge>
              {selected.issue.issueLink && (
                <Badge
                  tone={
                    selected.issue.issueLink.deliveryState === "CLOSED"
                      ? "success"
                      : selected.issue.issueLink.deliveryState === "FAILED" ||
                          selected.issue.issueLink.deliveryState ===
                            "BLOCKED" ||
                          selected.issue.issueLink.deliveryState === "PARTIAL"
                        ? "attention"
                        : "info"
                  }
                >
                  {t("issues.delivery.state", {
                    state: selected.issue.issueLink.deliveryState,
                  })}
                </Badge>
              )}
              {selected.issue.issueLink?.pullRequestUrl && (
                <a
                  href={selected.issue.issueLink.pullRequestUrl}
                  target="_blank"
                  rel="noreferrer"
                >
                  {t("issues.delivery.pullRequest", {
                    number: selected.issue.issueLink.pullRequestNumber ?? "",
                  })}
                </a>
              )}
              {selected.issue.issueLink?.mergeSha && (
                <code>
                  {t("issues.delivery.merge", {
                    sha: selected.issue.issueLink.mergeSha.slice(0, 12),
                  })}
                </code>
              )}
              <span>{selected.issue.author ?? t("common.unknown")}</span>
              <span>{formatTimestamp(selected.issue.updated_at)}</span>
              <a
                href={selected.issue.html_url}
                target="_blank"
                rel="noreferrer"
              >
                {t("issues.detail.github")}
              </a>
            </div>
            {selected.reason && (
              <p className="ui-callout ui-callout--attention">
                {selected.reason}
              </p>
            )}
            <div className="row">
              {selected.intakeRequestId && (
                <Link
                  className="ui-button ui-button--secondary ui-button--md"
                  to={`/requests/${selected.intakeRequestId}`}
                >
                  {t("issues.detail.openRequest")}
                </Link>
              )}
              {selected.issue.issueLink?.latestErrorEventId &&
                (selected.issue.issueLink.deliveryState === "FAILED" ||
                  selected.issue.issueLink.deliveryState === "BLOCKED") && (
                  <Button
                    variant="secondary"
                    disabled={busy !== null}
                    onClick={() => void retryDelivery(selected)}
                  >
                    {busy ===
                    `retry:${selected.issue.issueLink.id}:${selected.issue.issueLink.latestErrorEventId}`
                      ? t("issues.delivery.retrying")
                      : t("issues.delivery.retry")}
                  </Button>
                )}
              {(selected.state === "ready" || selected.state === "attention") &&
                (!selected.issue.issueLink ||
                  (selected.issue.issueLink.state === "NEEDS_ATTENTION" &&
                    selected.issue.issueLink.latestErrorEventId === null)) && (
                  <Button
                    variant="primary"
                    disabled={busy !== null}
                    onClick={() => void startIssue(selected)}
                  >
                    {busy?.startsWith("start:")
                      ? t("issues.start.starting")
                      : t("issues.start.action")}
                  </Button>
                )}
              {!boards?.find(
                (board) => board.project.id === selected.project.id,
              )?.intake?.policy?.enabled && (
                <Button
                  variant="secondary"
                  onClick={() => {
                    const board = boards?.find(
                      (entry) => entry.project.id === selected.project.id,
                    );
                    if (board) void openPolicy(board);
                    setSelected(null);
                  }}
                >
                  {t("issues.setup.action")}
                </Button>
              )}
            </div>
          </aside>
        </div>
      )}

      {policyProject && (
        <div className="issue-detail-scrim">
          <button
            type="button"
            className="issue-detail-backdrop"
            aria-label={t("issues.detail.close")}
            onClick={() => setPolicyProject(null)}
          />
          <aside
            className="issue-detail issue-policy"
            role="dialog"
            aria-modal="true"
            aria-labelledby="issue-policy-title"
          >
            <header className="issue-detail-head">
              <div>
                <p className="muted">
                  {projectDisplayName(policyProject.project)}
                </p>
                <h2 id="issue-policy-title">{t("issues.setup.title")}</h2>
              </div>
              <Button variant="quiet" onClick={() => setPolicyProject(null)}>
                {t("issues.detail.close")}
              </Button>
            </header>
            <p className="muted">{t("issues.setup.description")}</p>
            <form className="issues-policy-form" onSubmit={savePolicy}>
              <Input
                label={t("issues.intake.field.requiredLabels")}
                value={policyLabels}
                onChange={(event) => setPolicyLabels(event.target.value)}
                help={t("issues.intake.field.requiredLabelsHelp")}
              />
              <Input
                label={t("issues.intake.field.maxIssues")}
                type="number"
                min={1}
                max={20}
                value={policyMaxIssues}
                onChange={(event) => setPolicyMaxIssues(event.target.value)}
              />
              <Input
                label={t("issues.intake.field.policyExpiry")}
                type="datetime-local"
                value={policyExpiry}
                onChange={(event) => setPolicyExpiry(event.target.value)}
              />
              <Textarea
                label={t("issues.intake.field.reason")}
                value={policyReason}
                onChange={(event) => setPolicyReason(event.target.value)}
                rows={2}
              />
              <Checkbox
                label={t("issues.setup.acknowledge")}
                checked={policyAcknowledged}
                onChange={(event) =>
                  setPolicyAcknowledged(event.target.checked)
                }
              />
              <Button
                type="submit"
                variant="primary"
                disabled={busy === "policy"}
              >
                {busy === "policy"
                  ? t("issues.setup.saving")
                  : t("issues.setup.save")}
              </Button>
            </form>
          </aside>
        </div>
      )}
    </div>
  );
}
