import {
  ChevronLeft,
  FileText,
  Folder,
  Link2,
  Package,
  Search,
} from "lucide-react";
import {
  type KeyboardEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Link, useParams, useSearchParams } from "react-router-dom";
import { isMarkdownPath, Markdown } from "../components/Markdown";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
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
  type Project,
  type ProjectFileProjection,
  type ProjectRefsProjection,
  type ProjectTreeEntry,
  type ProjectTreeProjection,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

const MAX_RENDERED_LINES = 5_000;

function isCommitId(value: string): boolean {
  return /^[0-9a-f]{40}$|^[0-9a-f]{64}$/i.test(value);
}

function errorMessage(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function isStale(cause: unknown): boolean {
  return cause instanceof CoreApiError && cause.status === 409;
}

function formatBytes(value: number | null): string {
  if (value === null) return "—";
  if (value < 1_024) return `${value} B`;
  if (value < 1_024 * 1_024) return `${(value / 1_024).toFixed(1)} KiB`;
  return `${(value / (1_024 * 1_024)).toFixed(1)} MiB`;
}

function splitLines(content: string): string[] {
  if (content === "") return [];
  const lines = content.split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

function entryIcon(entry: ProjectTreeEntry) {
  if (entry.kind === "DIRECTORY")
    return <Folder size={16} aria-hidden="true" />;
  if (entry.kind === "SYMLINK") return <Link2 size={16} aria-hidden="true" />;
  if (entry.kind === "SUBMODULE")
    return <Package size={16} aria-hidden="true" />;
  return <FileText size={16} aria-hidden="true" />;
}

function entryKindLabel(
  kind: ProjectTreeEntry["kind"],
  t: ReturnType<typeof useI18n>["t"],
): string {
  if (kind === "DIRECTORY") return t("projectFiles.kind.directory");
  if (kind === "SYMLINK") return t("projectFiles.kind.symlink");
  if (kind === "SUBMODULE") return t("projectFiles.kind.submodule");
  return t("projectFiles.kind.file");
}

function pathSegments(path: string): { name: string; path: string }[] {
  const parts = path.split("/").filter(Boolean);
  return parts.map((name, index) => ({
    name,
    path: parts.slice(0, index + 1).join("/"),
  }));
}

function CodeViewer({ content, path }: { content: string; path: string }) {
  const { t } = useI18n();
  const lines = splitLines(content);
  const visible = lines.slice(0, MAX_RENDERED_LINES);
  let lineNumber = 0;
  return (
    <section
      className="project-code"
      aria-label={t("projectFiles.codeAria", { path })}
      // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrollable code region must be focusable so its overflow can be scrolled from the keyboard.
      tabIndex={0}
    >
      <div className="project-code-lines">
        {visible.map((line) => {
          lineNumber += 1;
          return (
            <div className="project-code-line" key={`line-${lineNumber}`}>
              <span className="project-code-number" aria-hidden="true">
                {lineNumber}
              </span>
              <span className="project-code-text">
                {line === "" ? " " : line}
              </span>
            </div>
          );
        })}
      </div>
      {lines.length > MAX_RENDERED_LINES && (
        <p className="project-code-note">
          {t("projectFiles.codeTruncated", {
            shown: MAX_RENDERED_LINES.toLocaleString(),
            total: lines.length.toLocaleString(),
          })}
        </p>
      )}
    </section>
  );
}

export function ProjectFiles() {
  const { t } = useI18n();
  const { projectId = "" } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const requestedRef = (searchParams.get("ref") ?? "").trim();
  const directoryPath = searchParams.get("path") ?? "";
  const filePath = searchParams.get("file") ?? "";

  const [project, setProject] = useState<Project | null>(null);
  const [refs, setRefs] = useState<ProjectRefsProjection | null>(null);
  const [tree, setTree] = useState<ProjectTreeProjection | null>(null);
  const [file, setFile] = useState<ProjectFileProjection | null>(null);
  const [missing, setMissing] = useState(false);
  const [stale, setStale] = useState(false);
  const [projectError, setProjectError] = useState<string | null>(null);
  const [treeError, setTreeError] = useState<string | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [loadingProject, setLoadingProject] = useState(true);
  const [loadingTree, setLoadingTree] = useState(false);
  const [loadingFile, setLoadingFile] = useState(false);
  const [filter, setFilter] = useState("");
  const [markdownMode, setMarkdownMode] = useState<"preview" | "source">(
    "preview",
  );
  const treeRef = useRef<HTMLDivElement>(null);

  const effectiveRef = isCommitId(requestedRef)
    ? requestedRef.toLowerCase()
    : (project?.base_commit ?? "");
  const invalidRevision = requestedRef !== "" && !isCommitId(requestedRef);

  useEffect(() => {
    let active = true;
    setLoadingProject(true);
    setProject(null);
    setRefs(null);
    setMissing(false);
    setStale(false);
    setProjectError(null);
    async function load() {
      try {
        const projects = await coreApi.listProjects();
        if (!active) return;
        const found = projects.find((entry) => entry.id === projectId);
        if (!found) {
          setMissing(true);
          return;
        }
        const loadedRefs = await coreApi.listProjectRefs(found.id);
        if (!active) return;
        setProject(found);
        setRefs(loadedRefs);
      } catch (cause) {
        if (!active) return;
        if (isStale(cause)) setStale(true);
        else
          setProjectError(errorMessage(cause, t("projectFiles.error.project")));
      } finally {
        if (active) setLoadingProject(false);
      }
    }
    void load();
    return () => {
      active = false;
    };
  }, [projectId, t]);

  useEffect(() => {
    if (!project || !refs || !effectiveRef) return;
    let active = true;
    setLoadingTree(true);
    setTree(null);
    setTreeError(null);
    void coreApi
      .listProjectFiles(project.id, {
        ref: effectiveRef,
        path: directoryPath,
      })
      .then((next) => {
        if (active) setTree(next);
      })
      .catch((cause) => {
        if (!active) return;
        if (isStale(cause)) setStale(true);
        else
          setTreeError(
            errorMessage(
              cause,
              directoryPath === ""
                ? t("projectFiles.error.root")
                : t("projectFiles.error.directory"),
            ),
          );
      })
      .finally(() => {
        if (active) setLoadingTree(false);
      });
    return () => {
      active = false;
    };
  }, [project, refs, effectiveRef, directoryPath, t]);

  useEffect(() => {
    if (!project || !refs || !filePath || !effectiveRef) {
      setFile(null);
      setFileError(null);
      return;
    }
    let active = true;
    setMarkdownMode("preview");
    setLoadingFile(true);
    setFile(null);
    setFileError(null);
    void coreApi
      .getProjectFile(project.id, { ref: effectiveRef, path: filePath })
      .then((next) => {
        if (active) setFile(next);
      })
      .catch((cause) => {
        if (!active) return;
        if (isStale(cause)) setStale(true);
        else setFileError(errorMessage(cause, t("projectFiles.error.file")));
      })
      .finally(() => {
        if (active) setLoadingFile(false);
      });
    return () => {
      active = false;
    };
  }, [project, refs, effectiveRef, filePath, t]);

  const branchNames = useMemo(() => {
    if (!refs) return [];
    return refs.refs
      .filter((ref) => ref.commit.toLowerCase() === effectiveRef)
      .map((ref) => ref.name);
  }, [refs, effectiveRef]);

  const refOptions = useMemo(() => {
    if (!project) return [];
    const options: { value: string; label: string }[] = [];
    const seen = new Set<string>();
    const base = project.base_commit.toLowerCase();
    options.push({
      value: base,
      label: t("projectFiles.ref.frozenBase", { id: shortId(base, 10) }),
    });
    seen.add(base);
    for (const ref of refs?.refs ?? []) {
      const commit = ref.commit.toLowerCase();
      if (seen.has(commit)) continue;
      seen.add(commit);
      options.push({
        value: commit,
        label: `${ref.name} · ${shortId(commit, 10)}`,
      });
    }
    if (effectiveRef && !seen.has(effectiveRef)) {
      options.push({
        value: effectiveRef,
        label: t("projectFiles.ref.commit", {
          id: shortId(effectiveRef, 10),
        }),
      });
    }
    return options;
  }, [project, refs, effectiveRef, t]);

  const visibleEntries = useMemo(() => {
    const query = filter.trim().toLowerCase();
    if (!tree) return [];
    if (!query) return tree.entries;
    return tree.entries.filter(
      (entry) =>
        entry.name.toLowerCase().includes(query) ||
        entry.path.toLowerCase().includes(query),
    );
  }, [tree, filter]);

  function navigate(next: {
    ref?: string;
    path?: string;
    file?: string;
  }): void {
    const params = new URLSearchParams();
    const nextRef = next.ref ?? effectiveRef;
    const nextPath = next.path ?? directoryPath;
    const nextFile = next.file ?? filePath;
    if (nextRef) params.set("ref", nextRef);
    if (nextPath) params.set("path", nextPath);
    if (nextFile) params.set("file", nextFile);
    setSearchParams(params);
  }

  function onEntryKeyDown(
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ): void {
    const buttons = Array.from(
      treeRef.current?.querySelectorAll<HTMLButtonElement>(
        "button[data-project-entry]",
      ) ?? [],
    );
    if (buttons.length === 0) return;
    let next = index;
    if (event.key === "ArrowDown")
      next = Math.min(index + 1, buttons.length - 1);
    else if (event.key === "ArrowUp") next = Math.max(index - 1, 0);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = buttons.length - 1;
    else return;
    event.preventDefault();
    buttons[next]?.focus();
  }

  if (missing) {
    return (
      <EmptyState
        title={t("projectFiles.notFoundTitle")}
        description={t("projectFiles.notFoundDescription", {
          id: projectId || t("projectFiles.identifierFallback"),
        })}
        action={<Link to="/projects">{t("projectFiles.back")}</Link>}
      />
    );
  }

  if (stale) {
    return (
      <ErrorState
        title={t("projectFiles.staleTitle")}
        message={t("projectFiles.staleDescription")}
        action={<Link to="/projects">{t("projectFiles.back")}</Link>}
      />
    );
  }

  if (loadingProject) return <LoadingState label={t("projectFiles.loading")} />;

  if (projectError && !project) {
    return (
      <ErrorState
        title={t("projectFiles.unavailableTitle")}
        message={projectError}
        action={
          <Button onClick={() => window.location.reload()}>
            {t("common.retry")}
          </Button>
        }
      />
    );
  }

  if (!project || !refs)
    return <LoadingState label={t("projectFiles.loading")} />;

  const selectedEntry = tree?.entries.find((entry) => entry.path === filePath);
  const baseRevision = project.base_commit.toLowerCase() === effectiveRef;
  const segments = pathSegments(directoryPath);

  return (
    <div className="project-files-workspace">
      <PageHeader
        title={t("projectFiles.title")}
        description={t("projectFiles.description")}
        meta={
          <>
            <Badge tone="accent" className="project-revision-badge">
              {t("projectFiles.meta.revision", {
                id: shortId(effectiveRef, 10),
              })}
            </Badge>
            {baseRevision && (
              <Badge tone="success">{t("projectFiles.meta.frozenBase")}</Badge>
            )}
            {branchNames.map((name) => (
              <Badge tone="info" key={name}>
                {name}
              </Badge>
            ))}
            <span title={effectiveRef}>
              {t("projectFiles.meta.commit")}{" "}
              <code>{shortId(effectiveRef, 12)}</code>
            </span>
            <span>
              {t("projectFiles.meta.identity")}{" "}
              <code title={project.identity ?? undefined}>
                {project.identity
                  ? shortId(project.identity, 12)
                  : t("common.notRecorded")}
              </code>
            </span>
          </>
        }
        actions={
          <Link className="ui-button ui-button--secondary" to="/projects">
            {t("projectFiles.back")}
          </Link>
        }
      />

      {invalidRevision && (
        <p className="ui-callout ui-callout--attention" role="status">
          {t("projectFiles.invalidRevision")}
        </p>
      )}

      <Panel className="project-files-controls">
        <nav aria-label={t("projectFiles.breadcrumb")}>
          <ol className="project-breadcrumb">
            <li>
              <button
                type="button"
                className="project-breadcrumb-link"
                onClick={() => navigate({ path: "", file: "" })}
                aria-current={directoryPath === "" ? "page" : undefined}
              >
                {t("projectFiles.repository")}
              </button>
            </li>
            {segments.map((segment, index) => (
              <li key={segment.path}>
                <span aria-hidden="true">/</span>
                <button
                  type="button"
                  className="project-breadcrumb-link"
                  onClick={() => navigate({ path: segment.path, file: "" })}
                  aria-current={
                    index === segments.length - 1 && filePath === ""
                      ? "page"
                      : undefined
                  }
                >
                  {segment.name}
                </button>
              </li>
            ))}
            {filePath && (
              <li>
                <span aria-hidden="true">/</span>
                <span className="project-breadcrumb-file" aria-current="page">
                  {filePath.split("/").pop()}
                </span>
              </li>
            )}
          </ol>
        </nav>
        <div className="project-files-toolbar">
          <label className="ui-field project-ref-select">
            <span className="ui-field-label">{t("projectFiles.revision")}</span>
            <select
              className="ui-select"
              value={effectiveRef}
              onChange={(event) =>
                navigate({
                  ref: event.target.value,
                  path: "",
                  file: "",
                })
              }
            >
              {refOptions.map((option) => (
                <option value={option.value} key={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>
          <label className="ui-field project-filter">
            <span className="ui-field-label">
              {t("projectFiles.filter.label")}
            </span>
            <span className="project-filter-control">
              <Search size={15} aria-hidden="true" />
              <input
                className="ui-input"
                type="search"
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
                placeholder={t("projectFiles.filter.placeholder")}
                aria-describedby="project-filter-count"
              />
            </span>
          </label>
        </div>
      </Panel>

      <div className="project-files-browser">
        <Panel className="project-files-tree" data-testid="project-files-tree">
          <div className="section-heading">
            <h2>{t("projectFiles.tree.title")}</h2>
            <span className="muted">
              {tree
                ? t("projectFiles.tree.entries", {
                    count: tree.entries.length,
                  })
                : "—"}
            </span>
          </div>
          {directoryPath !== "" && (
            <Button
              size="sm"
              variant="quiet"
              onClick={() =>
                navigate({
                  path: directoryPath.split("/").slice(0, -1).join("/"),
                  file: "",
                })
              }
            >
              <ChevronLeft size={14} aria-hidden="true" />{" "}
              {t("projectFiles.tree.parent")}
            </Button>
          )}
          {loadingTree && (
            <LoadingState label={t("projectFiles.tree.loading")} />
          )}
          {treeError && (
            <ErrorState
              title={t("projectFiles.tree.errorTitle")}
              message={treeError}
              action={
                <Button onClick={() => window.location.reload()}>
                  {t("common.retry")}
                </Button>
              }
            />
          )}
          {!loadingTree && !treeError && tree && (
            <>
              <p
                id="project-filter-count"
                className="project-filter-count"
                aria-live="polite"
              >
                {t("projectFiles.tree.count", {
                  shown: visibleEntries.length,
                  total: tree.entries.length,
                  filter: filter.trim(),
                })}
              </p>
              {tree.truncated && (
                <p className="ui-callout ui-callout--attention" role="status">
                  {t("projectFiles.tree.truncated")}
                </p>
              )}
              {tree.skippedEntries > 0 && (
                <p className="ui-callout ui-callout--attention" role="status">
                  {t("projectFiles.tree.skipped", {
                    count: tree.skippedEntries,
                  })}
                </p>
              )}
              {tree.entries.length === 0 ? (
                <EmptyState
                  title={t("projectFiles.tree.emptyTitle")}
                  description={t("projectFiles.tree.emptyDescription")}
                />
              ) : visibleEntries.length === 0 ? (
                <EmptyState
                  title={t("projectFiles.tree.noMatchTitle")}
                  description={t("projectFiles.tree.noMatchDescription")}
                  action={
                    <Button size="sm" onClick={() => setFilter("")}>
                      {t("projectFiles.tree.clearFilter")}
                    </Button>
                  }
                />
              ) : (
                <div className="project-entry-list" ref={treeRef}>
                  {visibleEntries.map((entry, index) => {
                    const selected = entry.path === filePath;
                    const isDirectory = entry.kind === "DIRECTORY";
                    return (
                      <button
                        key={entry.path}
                        type="button"
                        className={
                          selected
                            ? "project-entry project-entry--selected"
                            : "project-entry"
                        }
                        data-project-entry
                        aria-current={selected ? "true" : undefined}
                        aria-label={
                          isDirectory
                            ? t("projectFiles.entry.openDirectory", {
                                path: entry.path,
                              })
                            : t("projectFiles.entry.openFile", {
                                path: entry.path,
                              })
                        }
                        title={entry.path}
                        onKeyDown={(event) => onEntryKeyDown(event, index)}
                        onClick={() => {
                          if (isDirectory) {
                            setFilter("");
                            navigate({ path: entry.path, file: "" });
                          } else {
                            navigate({ file: entry.path });
                          }
                        }}
                      >
                        {entryIcon(entry)}
                        <span className="project-entry-name">{entry.name}</span>
                        <span className="project-entry-meta">
                          {isDirectory
                            ? t("projectFiles.kind.directory")
                            : formatBytes(entry.size)}
                        </span>
                      </button>
                    );
                  })}
                </div>
              )}
              <p className="project-provenance">{tree.provenance}</p>
            </>
          )}
        </Panel>

        <Panel
          className="project-files-viewer"
          data-testid="project-file-viewer"
        >
          {loadingFile && (
            <LoadingState label={t("projectFiles.file.loading")} />
          )}
          {fileError && (
            <ErrorState
              title={t("projectFiles.file.errorTitle")}
              message={fileError}
              action={
                <div className="project-file-error-actions">
                  <Button onClick={() => window.location.reload()}>
                    {t("common.retry")}
                  </Button>
                  <Button
                    variant="quiet"
                    onClick={() => navigate({ file: "" })}
                  >
                    {t("projectFiles.file.backToDirectory")}
                  </Button>
                </div>
              }
            />
          )}
          {!loadingFile && !fileError && !filePath && (
            <EmptyState
              title={t("projectFiles.file.selectTitle")}
              description={t("projectFiles.file.selectDescription")}
            />
          )}
          {!loadingFile && !fileError && filePath && !file && !treeError && (
            <EmptyState
              title={t("projectFiles.file.notSelectedTitle")}
              description={t("projectFiles.file.notSelectedDescription")}
            />
          )}
          {!loadingFile && !fileError && file && (
            <>
              <div className="section-heading project-file-heading">
                <h2>{file.name}</h2>
                <div className="project-file-badges">
                  <Badge>{entryKindLabel(file.kind, t)}</Badge>
                  {file.omitted === "BINARY" && (
                    <Badge tone="attention">
                      {t("projectFiles.file.binary")}
                    </Badge>
                  )}
                  {file.omitted === "TOO_LARGE" && (
                    <Badge tone="attention">
                      {t("projectFiles.file.large")}
                    </Badge>
                  )}
                  {file.omitted === "NOT_A_FILE" && (
                    <Badge tone="attention">
                      {t("projectFiles.file.notFollowed")}
                    </Badge>
                  )}
                  {isMarkdownPath(file.path) && (
                    <Badge tone="info">{t("projectFiles.file.markdown")}</Badge>
                  )}
                </div>
              </div>
              <dl className="detail project-file-meta">
                <dt>{t("projectFiles.file.path")}</dt>
                <dd>
                  <code>{file.path}</code>
                </dd>
                <dt>{t("projectFiles.file.revision")}</dt>
                <dd>
                  <code title={file.revision.commit}>
                    {shortId(file.revision.commit, 12)}
                  </code>
                </dd>
                <dt>{t("projectFiles.file.blob")}</dt>
                <dd>
                  <code title={file.objectId}>
                    {shortId(file.objectId, 12)}
                  </code>
                </dd>
                <dt>{t("projectFiles.file.mode")}</dt>
                <dd>
                  <code>{file.mode}</code>
                </dd>
                <dt>{t("projectFiles.file.size")}</dt>
                <dd>{formatBytes(file.size)}</dd>
                <dt>{t("projectFiles.file.lines")}</dt>
                <dd>{file.lineCount ?? "—"}</dd>
                <dt>{t("projectFiles.file.provenance")}</dt>
                <dd>{file.provenance}</dd>
              </dl>

              {file.omitted === "TOO_LARGE" && (
                <p className="ui-callout ui-callout--attention" role="status">
                  {t("projectFiles.file.tooLarge", {
                    size: formatBytes(file.size),
                  })}
                </p>
              )}
              {file.omitted === "BINARY" && (
                <p className="ui-callout ui-callout--attention" role="status">
                  {t("projectFiles.file.binaryDescription")}
                </p>
              )}
              {file.omitted === "NOT_A_FILE" && (
                <p className="ui-callout ui-callout--attention" role="status">
                  {file.kind === "SYMLINK"
                    ? t("projectFiles.file.symlinkDescription")
                    : t("projectFiles.file.submoduleDescription")}
                </p>
              )}

              {file.content === "" && (
                <EmptyState
                  title={t("projectFiles.file.emptyTitle")}
                  description={t("projectFiles.file.emptyDescription")}
                />
              )}

              {file.content !== null && file.content !== "" && (
                <>
                  {isMarkdownPath(file.path) && (
                    <fieldset className="project-view-toggle">
                      <legend className="ui-visually-hidden">
                        {t("projectFiles.file.rendering")}
                      </legend>
                      <Button
                        size="sm"
                        variant={
                          markdownMode === "preview" ? "primary" : "secondary"
                        }
                        aria-pressed={markdownMode === "preview"}
                        onClick={() => setMarkdownMode("preview")}
                      >
                        {t("projectFiles.file.preview")}
                      </Button>
                      <Button
                        size="sm"
                        variant={
                          markdownMode === "source" ? "primary" : "secondary"
                        }
                        aria-pressed={markdownMode === "source"}
                        onClick={() => setMarkdownMode("source")}
                      >
                        {t("projectFiles.file.source")}
                      </Button>
                    </fieldset>
                  )}
                  {isMarkdownPath(file.path) && markdownMode === "preview" ? (
                    <>
                      <p className="project-markdown-note">
                        {t("projectFiles.file.markdownNote")}
                      </p>
                      <Markdown source={file.content} />
                    </>
                  ) : (
                    <CodeViewer content={file.content} path={file.path} />
                  )}
                </>
              )}
            </>
          )}
        </Panel>
      </div>
      {selectedEntry && file && (
        <p className="project-selection-note">
          {t("projectFiles.selectedNote", {
            path: selectedEntry.path,
            revision: shortId(file.revision.commit, 12),
          })}
        </p>
      )}
    </div>
  );
}
