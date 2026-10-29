import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Checkbox, Input, Select, Textarea } from "../components/ui/Field";
import {
  EmptyState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  CoreApiError,
  coreApi,
  type ExecutionProfile,
  type IntakeExecutionPolicyScope,
  type IntakeRequestSummary,
  type NewIntakeRequest,
  newIdempotencyKey,
  type OfficeOverview,
  type Project,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";
import { IntakeModeBadge, IntakeStateBadge } from "./RequestState";

function errorMessage(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError
    ? cause.message
    : cause instanceof Error
      ? cause.message
      : fallback;
}

function endpointUnavailable(cause: unknown): boolean {
  return (
    cause instanceof CoreApiError &&
    (cause.status === 404 || cause.status === 405 || cause.status === 501)
  );
}

function policyScopeFromError(
  cause: CoreApiError,
): IntakeExecutionPolicyScope | null {
  const body =
    typeof cause.body === "object" && cause.body !== null
      ? (cause.body as { scope?: IntakeExecutionPolicyScope })
      : null;
  const scope = body?.scope;
  return scope && typeof scope === "object" ? scope : null;
}

function projectName(
  projectId: string,
  projects: Project[] | null,
  t: ReturnType<typeof useI18n>["t"],
): string {
  return (
    projects?.find((project) => project.id === projectId)?.repository ??
    t("home.projectFallback", { id: shortId(projectId) })
  );
}

export function Home() {
  const navigate = useNavigate();
  const { t, formatDate, formatNumber } = useI18n();
  const [projects, setProjects] = useState<Project[] | null>(null);
  const [projectsError, setProjectsError] = useState<string | null>(null);
  const [profiles, setProfiles] = useState<ExecutionProfile[] | null>(null);
  const [profilesError, setProfilesError] = useState<string | null>(null);
  const [profilesUnavailable, setProfilesUnavailable] = useState(false);
  const [overview, setOverview] = useState<OfficeOverview | null>(null);
  const [overviewError, setOverviewError] = useState<string | null>(null);
  const [recentRequests, setRecentRequests] = useState<
    IntakeRequestSummary[] | null
  >(null);
  const [recentError, setRecentError] = useState<string | null>(null);
  const [recentUnavailable, setRecentUnavailable] = useState(false);

  const [projectId, setProjectId] = useState("");
  const [profileId, setProfileId] = useState("");
  const [objective, setObjective] = useState("");
  const [spendUsd, setSpendUsd] = useState("5.00");
  const [fieldErrors, setFieldErrors] = useState<{
    project?: string;
    objective?: string;
    spend?: string;
  }>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [policySetup, setPolicySetup] = useState<{
    pending: NewIntakeRequest;
    scope: IntakeExecutionPolicyScope;
    policyKey: string;
  } | null>(null);
  const [policyAcknowledged, setPolicyAcknowledged] = useState(false);
  const [policyBusy, setPolicyBusy] = useState(false);
  const [policyError, setPolicyError] = useState<string | null>(null);
  const requestKey = useRef(newIdempotencyKey());

  const loadProjects = useCallback(async () => {
    try {
      const next = await coreApi.listProjects();
      setProjects(next);
      setProjectsError(null);
      setProjectId((current) =>
        current !== "" && next.some((project) => project.id === current)
          ? current
          : (next[0]?.id ?? ""),
      );
    } catch (cause) {
      setProjectsError(errorMessage(cause, t("home.projects.error")));
    }
  }, [t]);

  const loadProfiles = useCallback(async () => {
    setProfiles(null);
    setProfilesError(null);
    setProfilesUnavailable(false);
    try {
      const { profiles: next } = await coreApi.listExecutionProfiles();
      setProfiles(next);
      setProfileId((current) =>
        current !== "" && next.some((profile) => profile.id === current)
          ? current
          : "",
      );
    } catch (cause) {
      setProfiles([]);
      setProfileId("");
      if (endpointUnavailable(cause)) {
        setProfilesUnavailable(true);
      } else {
        setProfilesError(errorMessage(cause, t("home.profiles.error")));
      }
    }
  }, [t]);

  const loadOverview = useCallback(async () => {
    try {
      const next = await coreApi.getOverview();
      setOverview(next);
      setOverviewError(null);
    } catch (cause) {
      setOverviewError(errorMessage(cause, t("home.status.error")));
    }
  }, [t]);

  const loadRecent = useCallback(async () => {
    try {
      const next = await coreApi.listIntakeRequests({ limit: 8 });
      setRecentRequests(next);
      setRecentError(null);
      setRecentUnavailable(false);
    } catch (cause) {
      if (endpointUnavailable(cause)) {
        setRecentUnavailable(true);
        setRecentRequests(null);
        setRecentError(null);
      } else {
        setRecentError(errorMessage(cause, t("home.recent.error")));
      }
    }
  }, [t]);

  useEffect(() => {
    void loadProjects();
    void loadProfiles();
  }, [loadProjects, loadProfiles]);

  useEffect(() => {
    void loadOverview();
    const timer = setInterval(() => void loadOverview(), 5000);
    return () => clearInterval(timer);
  }, [loadOverview]);

  useEffect(() => {
    void loadRecent();
    const timer = setInterval(() => void loadRecent(), 8000);
    return () => clearInterval(timer);
  }, [loadRecent]);

  const selectedProfile = profiles?.find((profile) => profile.id === profileId);
  const profileUnavailable = selectedProfile?.executable === false;
  const pendingDecisions =
    overview?.recent_decisions.filter((decision) => decision.actionable) ?? [];

  function resetRequestKey() {
    requestKey.current = newIdempotencyKey();
    setFormError(null);
    setPolicySetup(null);
    setPolicyAcknowledged(false);
    setPolicyError(null);
  }

  async function submitRequest(input: NewIntakeRequest) {
    setBusy(true);
    setFormError(null);
    setPolicyError(null);
    try {
      const created = await coreApi.createIntakeRequest(input);
      setPolicySetup(null);
      setPolicyAcknowledged(false);
      navigate(`/requests/${encodeURIComponent(created.id)}`);
    } catch (cause) {
      if (
        cause instanceof CoreApiError &&
        cause.reason === "EXECUTION_POLICY_REQUIRED"
      ) {
        const scope = policyScopeFromError(cause);
        if (scope) {
          setPolicySetup({
            pending: {
              ...input,
              ...(scope.profile === null
                ? {}
                : { execution_profile_id: scope.profile.id }),
            },
            scope,
            policyKey: newIdempotencyKey(),
          });
          setPolicyAcknowledged(false);
          return;
        }
      }
      setFormError(
        endpointUnavailable(cause)
          ? t("home.request.endpointUnavailable")
          : errorMessage(cause, t("home.request.error")),
      );
    } finally {
      setBusy(false);
    }
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    const trimmed = objective.trim();
    const nextErrors: { project?: string; objective?: string; spend?: string } = {};
    const spendMicros = Math.round(Number(spendUsd) * 1_000_000);
    if (projectId === "") nextErrors.project = t("home.validation.project");
    if (trimmed === "") {
      nextErrors.objective = t("home.validation.objective");
    }
    if (!Number.isFinite(spendMicros) || spendMicros <= 0) {
      nextErrors.spend = t("home.validation.authorization");
    }
    setFieldErrors(nextErrors);
    if (nextErrors.project || nextErrors.objective || nextErrors.spend) {
      if (nextErrors.project) {
        document.getElementById("home-project")?.focus();
      } else if (nextErrors.objective) {
        document.getElementById("home-objective")?.focus();
      } else {
        document.getElementById("home-spend")?.focus();
      }
      return;
    }
    if (profileUnavailable) {
      setFormError(t("home.profile.notExecutableError"));
      document.getElementById("home-profile")?.focus();
      return;
    }

    const input: NewIntakeRequest = {
      project_id: projectId,
      objective: trimmed,
      ...(profileId === "" ? {} : { execution_profile_id: profileId }),
      mode: "REVIEW_PLAN",
      idempotency_key: requestKey.current,
      authorized_micros: Math.round(Number(spendUsd) * 1_000_000),
    };
    await submitRequest(input);
  }

  async function enablePolicy() {
    if (!policySetup?.scope.profile || !policySetup.scope.project) return;
    const { scope, pending, policyKey } = policySetup;
    const profile = scope.profile;
    const project = scope.project;
    if (profile === null || project === null) return;
    setPolicyBusy(true);
    setPolicyError(null);
    try {
      await coreApi.enableExecutionPolicy(pending.project_id, {
        execution_profile_id: profile.id,
        expected_revision: profile.revision,
        expected_revision_hash: profile.contentHash,
        project_config_hash: project.configHash,
        acknowledge: true,
        idempotency_key: policyKey,
      });
      await submitRequest(pending);
    } catch (cause) {
      setPolicyError(errorMessage(cause, t("home.policy.error")));
    } finally {
      setPolicyBusy(false);
    }
  }

  return (
    <div className="home">
      <PageHeader
        title={t("home.title")}
        description={t("home.description")}
        meta={
          <>
            <span>{t("home.meta.flow")}</span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>{t("home.meta.durable")}</span>
          </>
        }
      />

      <div className="home-layout">
        <div className="home-primary">
          {projectsError && (
            <div className="ui-callout ui-callout--danger" role="alert">
              <p>{projectsError}</p>
              <Button size="sm" onClick={() => void loadProjects()}>
                {t("home.projects.retry")}
              </Button>
            </div>
          )}

          <Panel className="home-composer">
            <div className="section-heading">
              <h2>{t("home.newRequest.title")}</h2>
              <span className="muted">{t("home.newRequest.hint")}</span>
            </div>

            {projects === null && projectsError === null && (
              <LoadingState label={t("home.projects.loading")} />
            )}

            {projects !== null && projects.length === 0 && (
              <EmptyState
                title={t("home.projects.emptyTitle")}
                description={t("home.projects.emptyDescription")}
                action={
                  <Link
                    className="ui-button ui-button--secondary"
                    to="/projects"
                  >
                    {t("home.projects.open")}
                  </Link>
                }
              />
            )}

            {projects !== null && projects.length > 0 && (
              <form
                className="home-request-form"
                noValidate
                onSubmit={(event) => void submit(event)}
              >
                <Select
                  id="home-project"
                  label={t("home.project.label")}
                  value={projectId}
                  onChange={(event) => {
                    setProjectId(event.target.value);
                    setFieldErrors((current) => ({
                      ...current,
                      project: undefined,
                    }));
                    resetRequestKey();
                  }}
                  error={fieldErrors.project}
                  help={t("home.project.help")}
                  required
                >
                  <option value="">{t("home.project.select")}</option>
                  {projects.map((project) => (
                    <option key={project.id} value={project.id}>
                      {project.repository}
                    </option>
                  ))}
                </Select>

                <Textarea
                  id="home-objective"
                  label={t("home.objective.label")}
                  value={objective}
                  onChange={(event) => {
                    setObjective(event.target.value);
                    setFieldErrors((current) => ({
                      ...current,
                      objective: undefined,
                    }));
                    resetRequestKey();
                  }}
                  placeholder={t("home.objective.placeholder")}
                  help={t("home.objective.help")}
                  error={fieldErrors.objective}
                  rows={5}
                  required
                />

                <Input
                  id="home-spend"
                  label={t("home.spend.label")}
                  type="number"
                  min="0.01"
                  step="0.01"
                  value={spendUsd}
                  onChange={(event) => {
                    setSpendUsd(event.target.value);
                    setFieldErrors((current) => ({
                      ...current,
                      spend: undefined,
                    }));
                    resetRequestKey();
                  }}
                  help={t("home.spend.help")}
                  error={fieldErrors.spend}
                  required
                />

                <Select
                  id="home-profile"
                  label={t("home.profile.label")}
                  value={profileId}
                  onChange={(event) => {
                    setProfileId(event.target.value);
                    resetRequestKey();
                  }}
                  help={t("home.profile.help")}
                  disabled={profiles === null}
                >
                  <option value="">
                    {profilesUnavailable
                      ? t("home.profile.coreDefaultUnavailable")
                      : profiles === null
                        ? t("home.profile.loading")
                        : t("home.profile.coreDefault")}
                  </option>
                  {(profiles ?? []).map((profile) => (
                    <option
                      key={profile.id}
                      value={profile.id}
                      disabled={!profile.executable}
                    >
                      {profile.name}
                      {profile.executable
                        ? ""
                        : t("home.profile.unavailableSuffix")}
                      {profile.globalDefault
                        ? t("home.profile.globalSuffix")
                        : ""}
                      {profile.projectDefaults.includes(projectId) && projectId
                        ? t("home.profile.projectSuffix")
                        : ""}
                    </option>
                  ))}
                </Select>

                {profilesUnavailable && (
                  <p className="ui-callout ui-callout--info" role="status">
                    {t("home.profiles.unavailable")}
                  </p>
                )}
                {profilesError && (
                  <div className="ui-callout ui-callout--danger" role="alert">
                    <p>{profilesError}</p>
                    <Button size="sm" onClick={() => void loadProfiles()}>
                      {t("home.profiles.retry")}
                    </Button>
                  </div>
                )}
                {profiles !== null &&
                  profiles.length === 0 &&
                  !profilesUnavailable &&
                  profilesError === null && (
                    <p className="ui-callout ui-callout--attention">
                      {t("home.profiles.empty")}
                    </p>
                  )}
                {selectedProfile && selectedProfile.warnings.length > 0 && (
                  <div className="ui-callout ui-callout--attention">
                    <p>
                      {t("home.profile.warnings", {
                        name: selectedProfile.name,
                      })}
                    </p>
                    <ul className="home-warning-list">
                      {selectedProfile.warnings.map((warning) => (
                        <li key={warning}>{warning}</li>
                      ))}
                    </ul>
                  </div>
                )}
                {profileUnavailable && (
                  <p className="ui-callout ui-callout--danger" role="alert">
                    {t("home.profile.notExecutable")}
                  </p>
                )}

                <p className="ui-callout ui-callout--info home-mode-help">
                  <strong>{t("home.approval.title")}</strong>{" "}
                  {t("home.approval.body")}
                </p>

                {policySetup && (
                  <div
                    className="ui-callout ui-callout--attention home-policy"
                    role="status"
                  >
                    <h3>{t("home.policy.title")}</h3>
                    <p>{t("home.policy.description")}</p>
                    {policySetup.scope.profile && policySetup.scope.project ? (
                      <dl className="request-facts">
                        <div>
                          <dt>{t("home.policy.profile")}</dt>
                          <dd>
                            {policySetup.scope.profile.name} r
                            {policySetup.scope.profile.revision} ·{" "}
                            <code title={policySetup.scope.profile.contentHash}>
                              {shortId(
                                policySetup.scope.profile.contentHash,
                                12,
                              )}
                            </code>
                          </dd>
                        </div>
                        <div>
                          <dt>{t("home.policy.project")}</dt>
                          <dd>
                            {policySetup.scope.project.repository} ·{" "}
                            <code title={policySetup.scope.project.configHash}>
                              {shortId(
                                policySetup.scope.project.configHash,
                                12,
                              )}
                            </code>
                          </dd>
                        </div>
                        <div>
                          <dt>{t("home.policy.allowedPaths")}</dt>
                          <dd>
                            {policySetup.scope.project.allowedPaths.join(
                              ", ",
                            ) || t("home.policy.none")}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("home.policy.capabilities")}</dt>
                          <dd>
                            {policySetup.scope.project.capabilities.join(
                              ", ",
                            ) || t("home.policy.none")}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("home.policy.implementerRoute")}</dt>
                          <dd>
                            {policySetup.scope.profile.implementer.provider}/
                            {policySetup.scope.profile.implementer.model}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("home.policy.objectiveScope")}</dt>
                          <dd>
                            {policySetup.scope.profile.grantTerms.objectiveClasses.join(
                              ", ",
                            )}{" "}
                            ·{" "}
                            {t("home.policy.risk", {
                              classes:
                                policySetup.scope.profile.grantTerms.riskClasses.join(
                                  ", ",
                                ),
                            })}{" "}
                            ·{" "}
                            {policySetup.scope.profile.grantTerms
                              .autonomousStart
                              ? t("home.policy.autonomous")
                              : t("home.policy.nonAutonomous")}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("home.policy.perRequest")}</dt>
                          <dd>
                            {t("home.policy.perRequestValue", {
                              attempts:
                                policySetup.scope.profile.grantTerms
                                  .maxAttemptsPerRequest,
                              sessions:
                                policySetup.scope.profile.grantTerms
                                  .maxSessionsPerRequest,
                              turns:
                                policySetup.scope.profile.grantTerms
                                  .maxTurnsPerRequest,
                              inputTokens:
                                policySetup.scope.profile.grantTerms
                                  .maxInputTokensPerRequest,
                              outputTokens:
                                policySetup.scope.profile.grantTerms
                                  .maxOutputTokensPerRequest,
                              costMicros:
                                policySetup.scope.profile.grantTerms
                                  .maxCostMicrosPerRequest,
                              timeout:
                                policySetup.scope.profile.grantTerms
                                  .maxTimeoutSecsPerRequest,
                              wallClock:
                                policySetup.scope.profile.grantTerms
                                  .maxWallClockSecsPerRequest,
                            })}
                          </dd>
                        </div>
                        <div>
                          <dt>{t("home.policy.cumulative")}</dt>
                          <dd>
                            {t("home.policy.cumulativeValue", {
                              requests:
                                policySetup.scope.profile.grantTerms
                                  .maxRequests,
                              sessions:
                                policySetup.scope.profile.grantTerms
                                  .maxCumulativeSessions,
                              attempts:
                                policySetup.scope.profile.grantTerms
                                  .maxCumulativeAttempts,
                              days: policySetup.scope.profile.grantTerms
                                .validityDays,
                              repairCycles:
                                policySetup.scope.profile.grantTerms
                                  .maxRepairCycles,
                              parallel:
                                policySetup.scope.profile.grantTerms
                                  .maxParallel,
                            })}
                          </dd>
                        </div>
                      </dl>
                    ) : (
                      <p className="ui-callout ui-callout--danger">
                        {t("home.policy.scopeMissing")}
                      </p>
                    )}
                    <Checkbox
                      label={t("home.policy.acknowledge")}
                      checked={policyAcknowledged}
                      onChange={(event) =>
                        setPolicyAcknowledged(event.target.checked)
                      }
                      disabled={
                        policyBusy ||
                        policySetup.scope.profile === null ||
                        policySetup.scope.project === null
                      }
                    />
                    {policyError && (
                      <p className="ui-callout ui-callout--danger" role="alert">
                        {policyError}
                      </p>
                    )}
                    <div className="request-form-actions">
                      <Button
                        type="button"
                        variant="primary"
                        disabled={
                          policyBusy ||
                          !policyAcknowledged ||
                          policySetup.scope.profile === null ||
                          policySetup.scope.project === null
                        }
                        onClick={() => void enablePolicy()}
                      >
                        {policyBusy
                          ? t("home.policy.enabling")
                          : t("home.policy.enable")}
                      </Button>
                    </div>
                  </div>
                )}

                {formError && (
                  <p className="ui-callout ui-callout--danger" role="alert">
                    {formError}
                  </p>
                )}

                <div className="home-form-actions">
                  <Button
                    type="submit"
                    variant="primary"
                    disabled={
                      busy || profileUnavailable || policySetup !== null
                    }
                  >
                    {busy ? t("home.submitting") : t("home.submit")}
                  </Button>
                  <span className="muted">{t("home.submitHint")}</span>
                </div>
              </form>
            )}
          </Panel>

          <Panel className="home-recent">
            <div className="section-heading">
              <h2>{t("home.recent.title")}</h2>
              <span className="muted">{t("home.recent.hint")}</span>
            </div>
            {recentRequests === null && !recentUnavailable && !recentError && (
              <LoadingState label={t("home.recent.loading")} />
            )}
            {recentUnavailable && (
              <p className="ui-callout ui-callout--info" role="status">
                {t("home.recent.unavailable")}
              </p>
            )}
            {recentError && (
              <div className="ui-callout ui-callout--danger" role="alert">
                <p>{recentError}</p>
                <Button size="sm" onClick={() => void loadRecent()}>
                  {t("home.recent.retry")}
                </Button>
              </div>
            )}
            {recentRequests !== null && recentRequests.length === 0 && (
              <EmptyState
                title={t("home.recent.emptyTitle")}
                description={t("home.recent.emptyDescription")}
              />
            )}
            {recentRequests !== null && recentRequests.length > 0 && (
              <ul className="home-request-list">
                {recentRequests.map((request) => (
                  <li key={request.id}>
                    <div className="home-request-copy">
                      <Link to={`/requests/${encodeURIComponent(request.id)}`}>
                        {request.objective}
                      </Link>
                      <span className="muted">
                        {projectName(request.projectId, projects, t)} ·{" "}
                        {formatDate(request.updatedAt)}
                      </span>
                    </div>
                    <div className="home-request-badges">
                      <IntakeStateBadge state={request.state} />
                      <IntakeModeBadge mode={request.mode} />
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </Panel>
        </div>

        <aside className="home-side" aria-label={t("home.status.aria")}>
          <Panel className="home-status">
            <div className="section-heading">
              <h2>{t("home.status.title")}</h2>
              {overview && (
                <Badge
                  tone={overview.runtime.core_healthy ? "success" : "danger"}
                >
                  {overview.runtime.core_healthy ? "HEALTHY" : "UNHEALTHY"}
                </Badge>
              )}
            </div>
            {overview === null && overviewError === null && (
              <LoadingState label={t("home.status.loading")} />
            )}
            {overview === null && overviewError !== null && (
              <div className="ui-callout ui-callout--danger" role="alert">
                <p>{t("home.status.unavailable", { error: overviewError })}</p>
                <Button size="sm" onClick={() => void loadOverview()}>
                  {t("home.status.retry")}
                </Button>
              </div>
            )}
            {overview && (
              <>
                <p className="muted home-status-time">
                  {t("home.status.snapshot", {
                    date: formatDate(overview.as_of),
                  })}
                </p>
                {overviewError && (
                  <div
                    className="ui-callout ui-callout--attention"
                    role="status"
                  >
                    <p>
                      {t("home.status.refreshFailed", {
                        error: overviewError,
                      })}
                    </p>
                    <Button size="sm" onClick={() => void loadOverview()}>
                      {t("home.status.retryNow")}
                    </Button>
                  </div>
                )}
                <dl className="home-facts">
                  <div>
                    <dt>{t("home.status.workers")}</dt>
                    <dd>{formatNumber(overview.runtime.workers)}</dd>
                  </div>
                  <div>
                    <dt>{t("home.status.unresolvedSessions")}</dt>
                    <dd>
                      {formatNumber(overview.runtime.unresolved_sessions)}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("home.status.unresolvedModelCalls")}</dt>
                    <dd>
                      {formatNumber(overview.runtime.unresolved_model_calls)}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("home.status.telemetry")}</dt>
                    <dd>
                      <Badge
                        tone={
                          overview.runtime.observability.enabled
                            ? "success"
                            : "muted"
                        }
                      >
                        {overview.runtime.observability.enabled
                          ? "ENABLED"
                          : "DISABLED"}
                      </Badge>
                    </dd>
                  </div>
                </dl>
              </>
            )}
          </Panel>

          <Panel className="home-active">
            <div className="section-heading">
              <h2>{t("home.active.title")}</h2>
              <Link to="/campaigns">{t("nav.campaigns")}</Link>
            </div>
            {overview === null ? (
              <p className="muted">{t("home.active.unavailable")}</p>
            ) : (
              <>
                <dl className="home-facts">
                  <div>
                    <dt>{t("home.active.authorized")}</dt>
                    <dd>
                      {formatNumber(overview.counts.authorized_campaigns)}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("home.active.awaiting")}</dt>
                    <dd>
                      {formatNumber(
                        overview.counts.campaigns_awaiting_authorization,
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("home.active.running")}</dt>
                    <dd>{formatNumber(overview.counts.active_agents)}</dd>
                  </div>
                  <div>
                    <dt>{t("home.active.blocked")}</dt>
                    <dd>{formatNumber(overview.counts.blocked_signals)}</dd>
                  </div>
                </dl>
                <h3 className="home-subheading">{t("home.active.events")}</h3>
                {overview.recent_events.length === 0 ? (
                  <p className="muted">{t("home.active.noEvents")}</p>
                ) : (
                  <ul className="home-events">
                    {overview.recent_events.slice(0, 3).map((event) => (
                      <li key={event.sequence}>
                        <code>#{event.sequence}</code>
                        <strong>{event.kind}</strong>
                        <span className="muted">
                          {formatDate(event.recorded_at)}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
          </Panel>

          <Panel className="home-decisions">
            <div className="section-heading">
              <h2>{t("home.decisions.title")}</h2>
              <Link to="/inbox">{t("home.decisions.open")}</Link>
            </div>
            {overview === null ? (
              <p className="muted">{t("home.active.unavailable")}</p>
            ) : pendingDecisions.length === 0 ? (
              <EmptyState
                title={t("home.decisions.noneTitle")}
                description={t("home.decisions.noneDescription")}
              />
            ) : (
              <ul className="home-decision-list">
                {pendingDecisions.slice(0, 4).map((decision) => (
                  <li key={decision.id}>
                    <Link to={decision.target}>{decision.title}</Link>
                    <Badge
                      tone={
                        decision.severity === "critical"
                          ? "danger"
                          : decision.severity === "attention"
                            ? "attention"
                            : "info"
                      }
                    >
                      {decision.severity}
                    </Badge>
                    <p className="muted">{decision.detail}</p>
                  </li>
                ))}
              </ul>
            )}
          </Panel>
        </aside>
      </div>
    </div>
  );
}
