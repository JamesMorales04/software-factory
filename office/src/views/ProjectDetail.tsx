import { ArrowLeft, RefreshCw, ScanSearch, Wrench } from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { Badge, type BadgeTone } from "../components/ui/Badge";
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
  type ProjectDiscoveryRecord,
  type ProjectSummary,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

function errorMessage(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function healthTone(health: ProjectSummary["github"]["health"]): BadgeTone {
  if (health === "CONNECTED") return "success";
  if (health === "REVOKED") return "danger";
  if (health === "NOT_CONNECTED") return "attention";
  return "muted";
}

export function ProjectDetail() {
  const { projectId = "" } = useParams();
  const { t, formatDate } = useI18n();
  const [summary, setSummary] = useState<ProjectSummary | null>(null);
  const [discovery, setDiscovery] = useState<ProjectDiscoveryRecord | null>(
    null,
  );
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);
  const [applying, setApplying] = useState(false);
  const [applyPaths, setApplyPaths] = useState("");

  useEffect(() => {
    let mounted = true;
    async function load() {
      setLoading(true);
      setError(null);
      try {
        const [nextSummary, nextDiscovery] = await Promise.all([
          coreApi.getProjectSummary(projectId),
          coreApi.getProjectDiscovery(projectId).catch(() => ({
            discovery: null,
            current_base_commit: "",
            stale: false,
          })),
        ]);
        if (!mounted) return;
        setSummary(nextSummary);
        setDiscovery(nextDiscovery.discovery);
        setApplyPaths(nextSummary.config.allowed_paths.join(", "));
      } catch (cause) {
        if (mounted) {
          setError(errorMessage(cause, t("projects.detail.error")));
        }
      } finally {
        if (mounted) setLoading(false);
      }
    }
    void load();
    return () => {
      mounted = false;
    };
  }, [projectId, t]);

  async function scan() {
    setScanning(true);
    setError(null);
    setNotice(null);
    try {
      const record = await coreApi.scanProjectDiscovery(projectId);
      setDiscovery(record);
      const nextSummary = await coreApi.getProjectSummary(projectId);
      setSummary(nextSummary);
      setNotice(
        t("projects.detail.scanNotice", {
          count: record.discovery.toolchains.length,
        }),
      );
    } catch (cause) {
      setError(errorMessage(cause, t("projects.detail.errorScan")));
    } finally {
      setScanning(false);
    }
  }

  async function applyCapabilities() {
    if (!summary) return;
    setApplying(true);
    setError(null);
    setNotice(null);
    try {
      const capabilities = new Set(summary.config.capabilities);
      for (const capability of summary.tooling?.suggested_capabilities ?? []) {
        capabilities.add(capability);
      }
      const next = await coreApi.updateProjectConfig(projectId, {
        expectedRevision: summary.config.revision,
        allowedPaths: applyPaths
          .split(",")
          .map((path) => path.trim())
          .filter(Boolean),
        capabilities: [...capabilities],
        qualityProfile: summary.config.quality_profile,
        active: summary.config.active,
      });
      setSummary({
        ...summary,
        config: {
          revision: next.revision,
          config_hash: next.configHash,
          allowed_paths: next.allowedPaths,
          capabilities: next.capabilities,
          quality_profile: next.qualityProfile,
          active: next.active,
          updated_at: next.updatedAt,
        },
      });
      setNotice(t("projects.detail.applied"));
    } catch (cause) {
      setError(errorMessage(cause, t("projects.detail.errorApply")));
    } finally {
      setApplying(false);
    }
  }

  if (loading) {
    return <LoadingState label={t("projects.detail.loading")} />;
  }
  if (error && !summary) {
    return (
      <ErrorState
        title={t("projects.detail.errorTitle")}
        message={error}
        action={
          <Link className="ui-button ui-button--secondary" to="/projects">
            {t("projects.detail.back")}
          </Link>
        }
      />
    );
  }
  if (!summary) {
    return (
      <EmptyState
        title={t("projects.detail.notFound")}
        action={<Link to="/projects">{t("projects.detail.back")}</Link>}
      />
    );
  }

  const project = summary.project;
  const binding = summary.github.binding;
  const ownerRepo =
    binding?.repo_full_name ||
    (project.source_kind === "github"
      ? project.source_url
          .replace(/^https:\/\/github\.com\//, "")
          .replace(/\.git$/, "")
      : "");

  return (
    <div className="projects-workspace project-detail">
      <Link className="project-detail-back" to="/projects">
        <ArrowLeft size={16} aria-hidden="true" /> {t("projects.detail.back")}
      </Link>
      <PageHeader
        title={project.name}
        description={
          ownerRepo ||
          project.remote_origin ||
          t("projects.detail.localRepository")
        }
        meta={
          <>
            <Badge tone="neutral">
              {project.source_kind === "github"
                ? "GitHub"
                : project.source_kind === "git"
                  ? "Git URL"
                  : "Local"}
            </Badge>
            {project.source_visibility && (
              <Badge tone="muted">{project.source_visibility}</Badge>
            )}
            <Badge tone={healthTone(summary.github.health)}>
              {summary.github.health}
            </Badge>
            {summary.config.active ? (
              <Badge tone="success">{t("projects.detail.active")}</Badge>
            ) : (
              <Badge tone="muted">{t("projects.detail.inactive")}</Badge>
            )}
          </>
        }
        actions={
          <Link
            className="ui-button ui-button--secondary ui-button--md"
            to={`/projects/${encodeURIComponent(project.id)}/files`}
          >
            {t("projects.detail.browseFiles")}
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

      <div className="project-detail-grid">
        <Panel>
          <h2>{t("projects.detail.sourceTitle")}</h2>
          <dl className="project-facts">
            <dt>{t("projects.detail.sourceUrl")}</dt>
            <dd>
              <code>{project.source_url || "—"}</code>
            </dd>
            <dt>{t("projects.detail.defaultBranch")}</dt>
            <dd>
              <code>{project.default_branch || "—"}</code>
            </dd>
            <dt>{t("projects.detail.baseCommit")}</dt>
            <dd>
              <code title={project.base_commit}>
                {shortId(project.base_commit, 12)}
              </code>
            </dd>
            <dt>{t("projects.detail.created")}</dt>
            <dd>{formatDate(project.created_at)}</dd>
            <dt>{t("projects.detail.lastActivity")}</dt>
            <dd>
              {project.last_activity_at
                ? formatDate(project.last_activity_at)
                : t("projects.detail.noActivity")}
            </dd>
            <dt>{t("projects.detail.remoteOrigin")}</dt>
            <dd>
              <code>{project.remote_origin || "—"}</code>
            </dd>
          </dl>
        </Panel>

        <Panel>
          <h2>{t("projects.detail.connection")}</h2>
          <div className="project-connection">
            <Badge tone={healthTone(summary.github.health)}>
              {summary.github.health}
            </Badge>
            {summary.github.health === "NOT_APPLICABLE" ? (
              <span className="muted">
                {t("projects.detail.connectionNotApplicable")}
              </span>
            ) : (
              <>
                {summary.github.connection.login && (
                  <span>
                    {t("projects.detail.login", {
                      login: summary.github.connection.login,
                    })}
                  </span>
                )}
                {summary.github.connection.configured && (
                  <>
                    <span className="muted">
                      {summary.github.connection.scopes_reported
                        ? t("projects.detail.scopes", {
                            scopes:
                              summary.github.connection.scopes.join(", ") ||
                              "none",
                          })
                        : t("projects.detail.scopesUnreported")}
                    </span>
                    <span className="muted">
                      {t("projects.detail.connectionRevision", {
                        revision: summary.github.connection.revision,
                      })}
                    </span>
                  </>
                )}
              </>
            )}
          </div>
          {binding ? (
            <dl className="project-facts">
              <dt>{t("projects.detail.binding")}</dt>
              <dd>
                <code>{binding.repo_full_name}</code>
              </dd>
              <dt>{t("projects.detail.boundAt")}</dt>
              <dd>{formatDate(binding.bound_at)}</dd>
              <dt>{t("projects.detail.sync")}</dt>
              <dd>
                <Badge
                  tone={
                    summary.github.sync_state?.status === "OK"
                      ? "success"
                      : summary.github.sync_state?.status === "NEVER_SYNCED"
                        ? "muted"
                        : "attention"
                  }
                >
                  {summary.github.sync_state?.status ?? "NEVER_SYNCED"}
                </Badge>{" "}
                <span className="muted">
                  {summary.github.sync_state?.detail ||
                    t("projects.detail.notSynced")}
                </span>
              </dd>
            </dl>
          ) : (
            <p className="muted">{t("projects.detail.notBound")}</p>
          )}
          {summary.github.health !== "NOT_APPLICABLE" && (
            <p className="muted">{summary.github.connection.detail}</p>
          )}
        </Panel>

        <Panel>
          <h2>{t("projects.detail.activityTitle")}</h2>
          <p className="muted">
            {t("projects.detail.activeCampaigns", {
              count: summary.campaigns.active_count,
            })}
          </p>
          {summary.campaigns.active.length === 0 ? (
            <p className="muted">{t("projects.detail.noActiveCampaigns")}</p>
          ) : (
            <ul className="project-activity-list">
              {summary.campaigns.active.map((campaign) => (
                <li key={campaign.id}>
                  <Link to={`/campaigns/${encodeURIComponent(campaign.id)}`}>
                    <code>{shortId(campaign.id)}</code>
                  </Link>{" "}
                  <Badge tone="attention">{campaign.status}</Badge>
                  <div className="muted project-clamp">
                    {campaign.objective}
                  </div>
                </li>
              ))}
            </ul>
          )}
          <p className="muted">
            {t("projects.detail.pendingDecisions", {
              count: summary.pending_decision_count,
            })}
          </p>
          {summary.pending_decisions.length === 0 ? (
            <p className="muted">{t("projects.detail.noPendingDecisions")}</p>
          ) : (
            <ul className="project-activity-list">
              {summary.pending_decisions.map((decision) => (
                <li key={decision.id}>
                  {decision.target.startsWith("/") ? (
                    <Link to={decision.target}>{decision.title}</Link>
                  ) : (
                    <span>{decision.title}</span>
                  )}
                  <div className="muted project-clamp">{decision.detail}</div>
                </li>
              ))}
            </ul>
          )}
        </Panel>

        <Panel>
          <h2>{t("projects.detail.capabilitiesTitle")}</h2>
          <div className="project-capabilities">
            {summary.config.capabilities.length === 0 ? (
              <p className="muted">{t("projects.detail.noCapabilities")}</p>
            ) : (
              summary.config.capabilities.map((capability) => (
                <Badge key={capability} tone="info">
                  {capability}
                </Badge>
              ))
            )}
          </div>
          <dl className="project-facts">
            <dt>{t("projects.detail.qualityProfile")}</dt>
            <dd>{summary.config.quality_profile}</dd>
            <dt>{t("projects.detail.configRevision")}</dt>
            <dd>{summary.config.revision}</dd>
            <dt>{t("projects.detail.allowedPaths")}</dt>
            <dd>
              <code>{summary.config.allowed_paths.join(", ") || "—"}</code>
            </dd>
          </dl>
        </Panel>
      </div>

      <Panel className="project-tooling" data-testid="project-tooling">
        <div className="project-tooling-head">
          <div>
            <h2>
              <Wrench size={18} aria-hidden="true" />{" "}
              {t("projects.detail.toolingTitle")}
            </h2>
            <p className="muted">{t("projects.detail.toolingDescription")}</p>
          </div>
          <Button
            variant="secondary"
            onClick={() => void scan()}
            disabled={scanning}
          >
            {scanning ? (
              <>
                <RefreshCw size={16} className="ui-spin" aria-hidden="true" />{" "}
                {t("projects.detail.scanning")}
              </>
            ) : (
              <>
                <ScanSearch size={16} aria-hidden="true" />{" "}
                {discovery
                  ? t("projects.detail.rescan")
                  : t("projects.detail.scan")}
              </>
            )}
          </Button>
        </div>
        {!discovery ? (
          <p className="muted">{t("projects.detail.noDiscovery")}</p>
        ) : (
          <>
            <p className="muted">
              {t("projects.detail.discoveryMeta", {
                commit: shortId(discovery.baseCommit, 12),
                date: formatDate(discovery.scannedAt),
              })}{" "}
              {discovery.baseCommit !== project.base_commit && (
                <Badge tone="attention">
                  {t("projects.detail.discoveryStale")}
                </Badge>
              )}
            </p>
            {discovery.discovery.toolchains.length === 0 ? (
              <p className="muted">{t("projects.detail.noToolchains")}</p>
            ) : (
              <ul className="project-toolchain-list">
                {discovery.discovery.toolchains.map((toolchain) => (
                  <li key={toolchain.id}>
                    <div>
                      <strong>{toolchain.label}</strong>{" "}
                      <code>{toolchain.id}</code>
                    </div>
                    <div className="muted">
                      {toolchain.manifests.join(", ")}
                    </div>
                    <div className="muted">
                      {toolchain.evidence.join(" · ")}
                    </div>
                  </li>
                ))}
              </ul>
            )}
            <p className="muted">
              {t("projects.detail.advisory")}{" "}
              <code>{discovery.discovery.provenance}</code>
            </p>
            {summary.tooling && (
              <>
                <h3>{t("projects.detail.suggestedCapabilities")}</h3>
                <div className="project-capabilities">
                  {summary.tooling.suggested_capabilities.map((capability) => (
                    <Badge key={capability} tone="accent">
                      {capability}
                    </Badge>
                  ))}
                </div>
                <label className="project-apply-paths">
                  {t("projects.detail.allowedPathsRequired")}
                  <input
                    value={applyPaths}
                    onChange={(event) => setApplyPaths(event.target.value)}
                    placeholder="src, tests"
                  />
                </label>
                <Button
                  variant="primary"
                  disabled={
                    applying ||
                    applyPaths
                      .split(",")
                      .map((path) => path.trim())
                      .filter(Boolean).length === 0
                  }
                  onClick={() => void applyCapabilities()}
                >
                  {applying
                    ? t("projects.detail.applying")
                    : t("projects.detail.applyCapabilities")}
                </Button>
                <h3>{t("projects.detail.suggestedChecks")}</h3>
                {summary.tooling.suggested_checks.length === 0 ? (
                  <p className="muted">
                    {t("projects.detail.noSuggestedChecks")}
                  </p>
                ) : (
                  <div className="table-responsive">
                    <table>
                      <thead>
                        <tr>
                          <th>{t("projects.detail.checkId")}</th>
                          <th>{t("projects.detail.checkKind")}</th>
                          <th>{t("projects.detail.checkCommand")}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {summary.tooling.suggested_checks.map((check) => (
                          <tr key={check.id}>
                            <td data-label={t("projects.detail.checkId")}>
                              <code>{check.id}</code>
                              <div className="muted">{check.description}</div>
                            </td>
                            <td data-label={t("projects.detail.checkKind")}>
                              {check.kind}
                            </td>
                            <td data-label={t("projects.detail.checkCommand")}>
                              <code>{check.command.join(" ")}</code>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </>
            )}
          </>
        )}
      </Panel>

      <details className="project-technical">
        <summary>{t("projects.detail.technical")}</summary>
        <dl className="project-facts">
          <dt>{t("projects.detail.projectId")}</dt>
          <dd>
            <code>{project.id}</code>
          </dd>
          <dt>{t("projects.detail.repositoryPath")}</dt>
          <dd>
            <code>{project.technical.repository_path}</code>
          </dd>
          <dt>{t("projects.detail.identity")}</dt>
          <dd>
            <code>{project.technical.identity}</code>
          </dd>
          <dt>{t("projects.detail.sourceKey")}</dt>
          <dd>
            <code>{project.technical.source_key}</code>
          </dd>
          <dt>{t("projects.detail.configHash")}</dt>
          <dd>
            <code>{summary.config.config_hash}</code>
          </dd>
        </dl>
      </details>
    </div>
  );
}
