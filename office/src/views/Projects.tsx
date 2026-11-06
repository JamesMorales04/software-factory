import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { EmptyState, PageHeader, Panel } from "../components/ui/Surface";
import {
  CoreApiError,
  coreApi,
  type GitHubRepository,
  newIdempotencyKey,
  type Project,
  type ProjectConfig,
  type ProjectImportOptions,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

type ImportTab = "github" | "local" | "url";

function projectName(project: Project): string {
  if (project.name?.trim()) return project.name;
  const source =
    project.source_url?.trim() ||
    project.repository.replace(/\/+$/, "") ||
    project.id;
  const candidate = source.split("/").filter(Boolean).at(-1) ?? source;
  return candidate.replace(/\.git$/, "") || project.id;
}

function sourceLabel(project: Project): string {
  if (project.source_kind === "github") return "GitHub";
  if (project.source_kind === "git") return "Git URL";
  return "Local";
}

function visibilityLabel(project: Project): string {
  if (project.source_visibility === "private") return "private";
  if (project.source_visibility === "public") return "public";
  return "";
}

function repositoryPermissionLabel(repository: GitHubRepository): string {
  if (repository.permissions?.push) return "push access";
  if (repository.permissions?.pull) return "read access";
  return "permissions not reported";
}

function errorMessage(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

export function Projects() {
  const { t, formatDate } = useI18n();
  const navigate = useNavigate();
  const [projects, setProjects] = useState<Project[]>([]);
  const [options, setOptions] = useState<ProjectImportOptions | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tab, setTab] = useState<ImportTab>("github");
  const [search, setSearch] = useState("");

  const [repositories, setRepositories] = useState<GitHubRepository[] | null>(
    null,
  );
  const [repositoryQuery, setRepositoryQuery] = useState("");
  const [repositoryBusy, setRepositoryBusy] = useState(false);
  const [repositoryAttempted, setRepositoryAttempted] = useState(false);
  const [repositoryError, setRepositoryError] = useState<string | null>(null);
  const [repositoryDetail, setRepositoryDetail] = useState("");
  const [importing, setImporting] = useState<string | null>(null);
  const [repositoryName, setRepositoryName] = useState("");

  const [localPath, setLocalPath] = useState("");
  const [localName, setLocalName] = useState("");
  const [url, setUrl] = useState("");
  const [urlName, setUrlName] = useState("");

  const [selected, setSelected] = useState<Project | null>(null);
  const [config, setConfig] = useState<ProjectConfig | null>(null);
  const [configPaths, setConfigPaths] = useState("");
  const [configCapabilities, setConfigCapabilities] = useState<string[]>([]);
  const [qualityProfile, setQualityProfile] = useState("generic");
  const [active, setActive] = useState(true);
  const attemptKey = useRef(newIdempotencyKey());

  async function refresh() {
    const next = await coreApi.listProjects();
    setProjects(next);
    return next;
  }

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const [next, nextOptions] = await Promise.all([
          coreApi.listProjects(),
          coreApi.getProjectImportOptions(),
        ]);
        if (mounted) {
          setProjects(next);
          setOptions(nextOptions);
        }
      } catch (cause) {
        if (mounted) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : t("projects.error.load"),
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => {
      void coreApi
        .listProjects()
        .then((next) => {
          if (mounted) setProjects(next);
        })
        .catch(() => undefined);
    }, 5000);
    return () => {
      mounted = false;
      clearInterval(timer);
    };
  }, [t]);

  useEffect(() => {
    if (
      tab !== "github" ||
      !options?.github.connected ||
      repositories !== null ||
      repositoryBusy ||
      repositoryAttempted
    ) {
      return;
    }
    let activeRequest = true;
    setRepositoryAttempted(true);
    setRepositoryBusy(true);
    setRepositoryError(null);
    void coreApi
      .listGitHubRepositories()
      .then((listing) => {
        if (!activeRequest) return;
        setRepositories(listing.repositories);
        setRepositoryDetail(listing.detail);
      })
      .catch((cause) => {
        if (activeRequest) {
          setRepositoryError(
            errorMessage(cause, t("projects.import.github.error")),
          );
        }
      })
      .finally(() => {
        if (activeRequest) setRepositoryBusy(false);
      });
    return () => {
      activeRequest = false;
    };
  }, [tab, options, repositories, repositoryBusy, repositoryAttempted, t]);

  async function loadRepositories(event?: React.FormEvent) {
    event?.preventDefault();
    setRepositoryBusy(true);
    setRepositoryError(null);
    setRepositoryDetail("");
    try {
      const listing = await coreApi.listGitHubRepositories(
        repositoryQuery.trim(),
      );
      setRepositories(listing.repositories);
      setRepositoryDetail(
        listing.rate_limited && listing.next_allowed_at
          ? t("projects.import.github.rateLimited", {
              until: formatDate(listing.next_allowed_at),
            })
          : listing.detail,
      );
    } catch (cause) {
      setRepositoryError(
        errorMessage(cause, t("projects.import.github.error")),
      );
    } finally {
      setRepositoryBusy(false);
    }
  }

  async function connectRepository(repository: GitHubRepository) {
    const [owner, repo] = repository.full_name.split("/");
    setImporting(repository.full_name);
    setError(null);
    setNotice(null);
    try {
      const result = await coreApi.importGitHubProject({
        owner,
        repo,
        name: repositoryName.trim() || undefined,
        default_branch: repository.default_branch,
        idempotency_key: newIdempotencyKey(),
      });
      setRepositoryName("");
      await refresh();
      setNotice(
        t("projects.import.github.notice", {
          name: result.project.name || repository.full_name,
        }),
      );
      navigate(`/projects/${encodeURIComponent(result.project.id)}`);
    } catch (cause) {
      setError(errorMessage(cause, t("projects.import.github.errorConnect")));
    } finally {
      setImporting(null);
    }
  }

  async function onRegisterLocal(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const project = await coreApi.registerProject({
        repository: localPath.trim(),
        name: localName.trim() || undefined,
        idempotency_key: attemptKey.current,
      });
      attemptKey.current = newIdempotencyKey();
      setLocalPath("");
      setLocalName("");
      await refresh();
      setNotice(
        t("projects.register.notice", {
          id: shortId(project.id),
          commit: shortId(project.base_commit, 12),
        }),
      );
      navigate(`/projects/${encodeURIComponent(project.id)}`);
    } catch (cause) {
      setError(errorMessage(cause, t("projects.error.register")));
    } finally {
      setBusy(false);
    }
  }

  async function onRegisterUrl(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const project = await coreApi.registerProject({
        repository: url.trim(),
        name: urlName.trim() || undefined,
        idempotency_key: attemptKey.current,
      });
      attemptKey.current = newIdempotencyKey();
      setUrl("");
      setUrlName("");
      await refresh();
      setNotice(
        t("projects.register.notice", {
          id: shortId(project.id),
          commit: shortId(project.base_commit, 12),
        }),
      );
      navigate(`/projects/${encodeURIComponent(project.id)}`);
    } catch (cause) {
      setError(errorMessage(cause, t("projects.error.register")));
    } finally {
      setBusy(false);
    }
  }

  const filteredProjects = useMemo(() => {
    const needle = search.trim().toLowerCase();
    if (!needle) return projects;
    return projects.filter((project) =>
      [
        projectName(project),
        project.repository,
        project.source_url,
        project.default_branch,
      ]
        .filter(Boolean)
        .join(" ")
        .toLowerCase()
        .includes(needle),
    );
  }, [projects, search]);

  return (
    <div className="projects-workspace">
      <PageHeader
        title={t("projects.title")}
        description={t("projects.description")}
        meta={
          <Badge tone="neutral">
            {projects.length === 1
              ? t("projects.countOne", { count: projects.length })
              : t("projects.count", { count: projects.length })}
          </Badge>
        }
        actions={
          <Link
            className="ui-button ui-button--secondary ui-button--md"
            to="/issues"
          >
            {t("projects.openIssues")}
          </Link>
        }
      />
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      <Panel className="project-import" data-testid="project-import">
        <div className="project-import-head">
          <div>
            <h2>{t("projects.import.title")}</h2>
            <p className="muted">{t("projects.import.description")}</p>
          </div>
        </div>
        <div
          className="segmented"
          role="tablist"
          aria-label={t("projects.import.title")}
        >
          {(
            [
              ["github", t("projects.import.tab.github")],
              ["local", t("projects.import.tab.local")],
              ["url", t("projects.import.tab.url")],
            ] as const
          ).map(([value, label]) => (
            <button
              key={value}
              type="button"
              role="tab"
              aria-selected={tab === value}
              className={tab === value ? "is-selected" : undefined}
              onClick={() => {
                setTab(value);
                setError(null);
                setNotice(null);
              }}
            >
              {label}
            </button>
          ))}
        </div>

        {tab === "github" && (
          <section role="tabpanel" aria-label={t("projects.import.tab.github")}>
            {!options ? (
              <p className="muted">{t("projects.import.loading")}</p>
            ) : !options.github.connected ? (
              <div className="ui-callout ui-callout--attention">
                <strong>{t("projects.import.github.disconnectedTitle")}</strong>
                <p>{t("projects.import.github.disconnectedBody")}</p>
                <Link to="/issues">
                  {t("projects.import.github.openSettings")}
                </Link>
              </div>
            ) : (
              <>
                <div className="project-connection">
                  <Badge tone="success">
                    {t("projects.import.github.connected")}
                  </Badge>
                  <span>
                    {t("projects.import.github.login", {
                      login: options.github.login ?? "unknown",
                    })}
                  </span>
                  <span className="muted">
                    {options.github.scopes_reported
                      ? t("projects.import.github.scopes", {
                          scopes: options.github.scopes.join(", ") || "none",
                        })
                      : t("projects.import.github.scopesUnreported")}
                  </span>
                  <span className="muted">
                    {t("projects.import.github.revision", {
                      revision: options.github.revision,
                    })}
                  </span>
                </div>
                <p className="muted">{options.github.fine_grained_detail}</p>
                {!options.github.app_oauth_available && (
                  <p className="muted">
                    {t("projects.import.github.appUnavailable")}
                  </p>
                )}
                <form
                  className="project-repo-search"
                  onSubmit={loadRepositories}
                >
                  <label>
                    {t("projects.import.github.searchLabel")}
                    <input
                      value={repositoryQuery}
                      onChange={(event) =>
                        setRepositoryQuery(event.target.value)
                      }
                      placeholder={t(
                        "projects.import.github.searchPlaceholder",
                      )}
                      maxLength={100}
                    />
                  </label>
                  <Button
                    type="submit"
                    variant="secondary"
                    disabled={repositoryBusy}
                  >
                    {repositoryBusy
                      ? t("projects.import.github.loading")
                      : t("projects.import.github.search")}
                  </Button>
                </form>
                {repositoryError && (
                  <p className="error" role="alert">
                    {repositoryError}
                  </p>
                )}
                {repositoryDetail && (
                  <p className="muted">{repositoryDetail}</p>
                )}
                {repositories !== null && repositories.length === 0 ? (
                  <EmptyState
                    title={t("projects.import.github.empty")}
                    description={t("projects.import.github.emptyDescription")}
                  />
                ) : null}
                {repositories !== null && repositories.length > 0 && (
                  <ul className="project-repository-list">
                    {repositories.map((repository) => (
                      <li key={repository.full_name}>
                        <div className="project-repository-main">
                          <strong>{repository.full_name}</strong>
                          <span className="project-repository-meta">
                            <Badge
                              tone={
                                repository.private ? "attention" : "neutral"
                              }
                            >
                              {repository.private
                                ? t("projects.visibility.private")
                                : t("projects.visibility.public")}
                            </Badge>
                            <span>
                              {t("projects.import.github.defaultBranch")}{" "}
                              <code>{repository.default_branch || "—"}</code>
                            </span>
                            <span>{repositoryPermissionLabel(repository)}</span>
                            {repository.pushed_at && (
                              <span>
                                {t("projects.import.github.pushed", {
                                  date: formatDate(repository.pushed_at),
                                })}
                              </span>
                            )}
                          </span>
                        </div>
                        <Button
                          variant="primary"
                          size="sm"
                          disabled={importing !== null}
                          onClick={() => void connectRepository(repository)}
                        >
                          {importing === repository.full_name
                            ? t("projects.import.github.connecting")
                            : t("projects.import.github.connect")}
                        </Button>
                      </li>
                    ))}
                  </ul>
                )}
                <label className="project-import-name">
                  {t("projects.import.name")}
                  <input
                    value={repositoryName}
                    onChange={(event) => setRepositoryName(event.target.value)}
                    placeholder={t("projects.import.namePlaceholder")}
                    maxLength={200}
                  />
                </label>
              </>
            )}
          </section>
        )}

        {tab === "local" && (
          <section role="tabpanel" aria-label={t("projects.import.tab.local")}>
            {!options ? (
              <p className="muted">{t("projects.import.loading")}</p>
            ) : !options.local.enabled ? (
              <div className="ui-callout ui-callout--attention">
                <strong>{t("projects.import.local.disabledTitle")}</strong>
                <p>{options.local.detail}</p>
              </div>
            ) : (
              <form className="create" onSubmit={onRegisterLocal}>
                <p className="muted">{options.local.detail}</p>
                <label>
                  {t("projects.import.local.path")}
                  <input
                    data-testid="project-local-path"
                    value={localPath}
                    onChange={(event) => {
                      setLocalPath(event.target.value);
                      attemptKey.current = newIdempotencyKey();
                    }}
                    placeholder={
                      options.local.managed_root ?? "/projects/checkout"
                    }
                    required
                  />
                </label>
                <label>
                  {t("projects.import.name")}
                  <input
                    value={localName}
                    onChange={(event) => setLocalName(event.target.value)}
                    placeholder={t("projects.import.namePlaceholder")}
                    maxLength={200}
                  />
                </label>
                <Button
                  type="submit"
                  variant="primary"
                  disabled={busy || localPath.trim() === ""}
                >
                  {busy
                    ? t("projects.import.local.busy")
                    : t("projects.import.local.submit")}
                </Button>
              </form>
            )}
          </section>
        )}

        {tab === "url" && (
          <section role="tabpanel" aria-label={t("projects.import.tab.url")}>
            <form className="create" onSubmit={onRegisterUrl}>
              <p className="muted">{t("projects.import.url.help")}</p>
              <label>
                {t("projects.import.url.label")}
                <input
                  data-testid="project-repository"
                  value={url}
                  onChange={(event) => {
                    setUrl(event.target.value);
                    attemptKey.current = newIdempotencyKey();
                  }}
                  placeholder="https://github.com/owner/repository.git"
                  required
                />
              </label>
              <label>
                {t("projects.import.name")}
                <input
                  value={urlName}
                  onChange={(event) => setUrlName(event.target.value)}
                  placeholder={t("projects.import.namePlaceholder")}
                  maxLength={200}
                />
              </label>
              <Button
                type="submit"
                variant="primary"
                disabled={busy || url.trim() === ""}
              >
                {busy
                  ? t("projects.register.busy")
                  : t("projects.register.submit")}
              </Button>
            </form>
          </section>
        )}
      </Panel>

      <section
        className="project-list-section"
        aria-labelledby="projects-list-title"
      >
        <div className="project-list-head">
          <h2 id="projects-list-title">
            {t("projects.list.title", { count: filteredProjects.length })}
          </h2>
          {projects.length > 0 && (
            <label className="project-search">
              <span className="ui-visually-hidden">
                {t("projects.search.label")}
              </span>
              <input
                type="search"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={t("projects.search.placeholder")}
                aria-label={t("projects.search.label")}
              />
            </label>
          )}
        </div>
        {projects.length === 0 ? (
          <EmptyState
            title={t("projects.list.empty")}
            description={t("projects.list.emptyDescription")}
            action={
              <Button variant="primary" onClick={() => setTab("github")}>
                {t("projects.list.emptyAction")}
              </Button>
            }
          />
        ) : filteredProjects.length === 0 ? (
          <EmptyState
            title={t("projects.list.noMatches")}
            description={t("projects.search.placeholder")}
            action={
              <Button onClick={() => setSearch("")}>
                {t("projects.search.clear")}
              </Button>
            }
          />
        ) : (
          <section
            className="table-responsive"
            aria-label={t("projects.table.label")}
          >
            <table>
              <thead>
                <tr>
                  <th>{t("projects.table.project")}</th>
                  <th>{t("projects.table.source")}</th>
                  <th>{t("projects.table.branch")}</th>
                  <th>{t("projects.table.registered")}</th>
                  <th>{t("projects.table.actions")}</th>
                </tr>
              </thead>
              <tbody>
                {filteredProjects.map((project) => (
                  <tr key={project.id}>
                    <td data-label={t("projects.table.project")}>
                      <Link
                        className="project-name-link"
                        to={`/projects/${encodeURIComponent(project.id)}`}
                      >
                        {projectName(project)}
                      </Link>
                      <div className="muted project-source-line">
                        {project.source_url || project.repository}
                      </div>
                    </td>
                    <td data-label={t("projects.table.source")}>
                      <Badge
                        tone={
                          project.source_kind === "github" ? "info" : "neutral"
                        }
                      >
                        {sourceLabel(project)}
                      </Badge>{" "}
                      {visibilityLabel(project) && (
                        <span className="muted">
                          {visibilityLabel(project)}
                        </span>
                      )}
                    </td>
                    <td data-label={t("projects.table.branch")}>
                      <code>{project.default_branch || "—"}</code>
                    </td>
                    <td
                      className="muted"
                      data-label={t("projects.table.registered")}
                    >
                      {formatDate(project.created_at)}
                    </td>
                    <td
                      className="project-actions"
                      data-label={t("projects.table.actions")}
                    >
                      <Link
                        className="ui-button ui-button--secondary ui-button--sm"
                        to={`/projects/${encodeURIComponent(project.id)}`}
                      >
                        {t("projects.detail.open")}
                      </Link>
                      <Link
                        className="ui-button ui-button--secondary ui-button--sm"
                        to={`/projects/${encodeURIComponent(project.id)}/files`}
                      >
                        {t("projects.browseFiles")}
                      </Link>
                      <button
                        type="button"
                        onClick={() => {
                          setSelected(project);
                          setError(null);
                          void coreApi
                            .getProjectConfig(project.id)
                            .then((next) => {
                              setConfig(next);
                              setConfigPaths(next.allowedPaths.join(", "));
                              setConfigCapabilities(next.capabilities);
                              setQualityProfile(next.qualityProfile);
                              setActive(next.active);
                            })
                            .catch((cause) =>
                              setError(
                                errorMessage(
                                  cause,
                                  t("projects.error.configLoad"),
                                ),
                              ),
                            );
                        }}
                      >
                        {t("projects.configure")}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        )}
      </section>

      {selected && config && (
        <form
          className="create"
          onSubmit={(event) => {
            event.preventDefault();
            setBusy(true);
            setError(null);
            void coreApi
              .updateProjectConfig(selected.id, {
                expectedRevision: config.revision,
                allowedPaths: configPaths
                  .split(",")
                  .map((path) => path.trim())
                  .filter(Boolean),
                capabilities: configCapabilities,
                qualityProfile,
                active,
              })
              .then((next) => {
                setConfig(next);
                setNotice(
                  t("projects.config.notice", {
                    revision: next.revision,
                    hash: next.configHash.slice(0, 12),
                  }),
                );
              })
              .catch((cause) =>
                setError(errorMessage(cause, t("projects.error.configSave"))),
              )
              .finally(() => setBusy(false));
          }}
        >
          <h3>{t("projects.config.title", { id: shortId(selected.id) })}</h3>
          <p className="muted">
            {t("projects.config.revision", { revision: config.revision })} ·{" "}
            {t("projects.config.base")}{" "}
            <code>{shortId(config.baseCommit, 12)}</code> ·{" "}
            {t("projects.config.hash")}{" "}
            <code>{shortId(config.configHash, 12)}</code>
          </p>
          <label>
            {t("projects.config.allowedPaths")}
            <input
              value={configPaths}
              onChange={(event) => setConfigPaths(event.target.value)}
              placeholder={t("projects.config.allowedPathsPlaceholder")}
            />
          </label>
          <fieldset>
            <legend>{t("projects.config.capabilities")}</legend>
            {[
              "repository-edit",
              "generic-toolchain",
              "node-toolchain",
              "browser",
            ].map((capability) => (
              <label className="inline" key={capability}>
                <input
                  type="checkbox"
                  checked={configCapabilities.includes(capability)}
                  onChange={(event) =>
                    setConfigCapabilities((current) =>
                      event.target.checked
                        ? [...current, capability]
                        : current.filter((item) => item !== capability),
                    )
                  }
                />
                {capability}
              </label>
            ))}
          </fieldset>
          <div className="grid-2">
            <label>
              {t("projects.config.qualityProfile")}
              <select
                value={qualityProfile}
                onChange={(event) => setQualityProfile(event.target.value)}
              >
                <option value="generic">generic</option>
                <option value="node-basic">node-basic</option>
                <option value="node-browser">node-browser</option>
              </select>
            </label>
            <label className="inline">
              <input
                type="checkbox"
                checked={active}
                onChange={(event) => setActive(event.target.checked)}
              />
              {t("projects.config.active")}
            </label>
          </div>
          <button type="submit" disabled={busy}>
            {busy ? t("projects.config.saving") : t("projects.config.save")}
          </button>
        </form>
      )}
    </div>
  );
}
