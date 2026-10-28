import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Checkbox, Select, Textarea } from "../components/ui/Field";
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
import { formatTimestamp, shortId } from "../format";
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

function projectName(projectId: string, projects: Project[] | null): string {
  return (
    projects?.find((project) => project.id === projectId)?.repository ??
    `Project ${shortId(projectId)}`
  );
}

export function Home() {
  const navigate = useNavigate();
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
  const [fieldErrors, setFieldErrors] = useState<{
    project?: string;
    objective?: string;
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
      setProjectsError(
        errorMessage(cause, "Projects could not be loaded from Core."),
      );
    }
  }, []);

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
        setProfilesError(
          errorMessage(cause, "Execution profiles could not be loaded."),
        );
      }
    }
  }, []);

  const loadOverview = useCallback(async () => {
    try {
      const next = await coreApi.getOverview();
      setOverview(next);
      setOverviewError(null);
    } catch (cause) {
      setOverviewError(
        errorMessage(cause, "Core status could not be refreshed."),
      );
    }
  }, []);

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
        setRecentError(
          errorMessage(cause, "Recent requests could not be loaded."),
        );
      }
    }
  }, []);

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
          ? "Unified request intake is not available from this Core build. The typed Office client is ready, but Core has not exposed /intake/requests."
          : errorMessage(cause, "The request could not be submitted."),
      );
    } finally {
      setBusy(false);
    }
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    const trimmed = objective.trim();
    const nextErrors: { project?: string; objective?: string } = {};
    if (projectId === "") nextErrors.project = "Select a Project.";
    if (trimmed === "") {
      nextErrors.objective = "Describe the engineering outcome you want.";
    }
    setFieldErrors(nextErrors);
    if (nextErrors.project || nextErrors.objective) {
      if (nextErrors.project) {
        document.getElementById("home-project")?.focus();
      } else {
        document.getElementById("home-objective")?.focus();
      }
      return;
    }
    if (profileUnavailable) {
      setFormError(
        "The selected execution profile is not executable in this Core build. Choose another profile.",
      );
      document.getElementById("home-profile")?.focus();
      return;
    }

    const input: NewIntakeRequest = {
      project_id: projectId,
      objective: trimmed,
      ...(profileId === "" ? {} : { execution_profile_id: profileId }),
      mode: "REVIEW_PLAN",
      idempotency_key: requestKey.current,
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
      setPolicyError(
        errorMessage(cause, "The execution policy could not be enabled."),
      );
    } finally {
      setPolicyBusy(false);
    }
  }

  return (
    <div className="home">
      <PageHeader
        title="Home"
        description="Describe the engineering outcome. Core derives the Action, Lead request, plan and Campaign authorization; Office never grants that authority."
        meta={
          <>
            <span>Project → request → profile → mode</span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>Requests are durable Core records</span>
          </>
        }
      />

      <div className="home-layout">
        <div className="home-primary">
          {projectsError && (
            <div className="ui-callout ui-callout--danger" role="alert">
              <p>{projectsError}</p>
              <Button size="sm" onClick={() => void loadProjects()}>
                Retry projects
              </Button>
            </div>
          )}

          <Panel className="home-composer">
            <div className="section-heading">
              <h2>New request</h2>
              <span className="muted">Operator intent, not an Action spec</span>
            </div>

            {projects === null && projectsError === null && (
              <LoadingState label="Loading Projects…" />
            )}

            {projects !== null && projects.length === 0 && (
              <EmptyState
                title="Register a Project first"
                description="A request is bound to an active Core Project. Registration freezes a base commit and does not grant execution authority."
                action={
                  <Link
                    className="ui-button ui-button--secondary"
                    to="/projects"
                  >
                    Open Projects
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
                  label="Project"
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
                  help="Core enforces the Project configuration, allowed paths and policy revision."
                  required
                >
                  <option value="">Select a Project</option>
                  {projects.map((project) => (
                    <option key={project.id} value={project.id}>
                      {project.repository}
                    </option>
                  ))}
                </Select>

                <Textarea
                  id="home-objective"
                  label="What should the Factory do?"
                  value={objective}
                  onChange={(event) => {
                    setObjective(event.target.value);
                    setFieldErrors((current) => ({
                      ...current,
                      objective: undefined,
                    }));
                    resetRequestKey();
                  }}
                  placeholder="Add a CSV export endpoint to the reporting API, keep the existing API backward compatible, and cover it with tests."
                  help="Plain language is enough. Core freezes the concrete plan and checks after assessment."
                  error={fieldErrors.objective}
                  rows={5}
                  required
                />

                <Select
                  id="home-profile"
                  label="Execution profile"
                  value={profileId}
                  onChange={(event) => {
                    setProfileId(event.target.value);
                    resetRequestKey();
                  }}
                  help="Profiles bind provider/model routes, budgets and limits. Core freezes the exact revision; every intake still requires explicit operator approval."
                  disabled={profiles === null}
                >
                  <option value="">
                    {profilesUnavailable
                      ? "Core default (profile list unavailable)"
                      : profiles === null
                        ? "Loading profiles…"
                        : "Core default (Project, then global)"}
                  </option>
                  {(profiles ?? []).map((profile) => (
                    <option
                      key={profile.id}
                      value={profile.id}
                      disabled={!profile.executable}
                    >
                      {profile.name}
                      {profile.executable ? "" : " — unavailable"}
                      {profile.globalDefault ? " · global default" : ""}
                      {profile.projectDefaults.includes(projectId) && projectId
                        ? " · project default"
                        : ""}
                    </option>
                  ))}
                </Select>

                {profilesUnavailable && (
                  <p className="ui-callout ui-callout--info" role="status">
                    Execution profiles are not available from this Core build.
                    Submitting without an explicit profile lets Core resolve its
                    Project or global default; if none exists, Core rejects the
                    request.
                  </p>
                )}
                {profilesError && (
                  <div className="ui-callout ui-callout--danger" role="alert">
                    <p>{profilesError}</p>
                    <Button size="sm" onClick={() => void loadProfiles()}>
                      Retry profiles
                    </Button>
                  </div>
                )}
                {profiles !== null &&
                  profiles.length === 0 &&
                  !profilesUnavailable &&
                  profilesError === null && (
                    <p className="ui-callout ui-callout--attention">
                      No execution profiles are configured. Core resolves a
                      default when one exists; otherwise it rejects the request.
                    </p>
                  )}
                {selectedProfile && selectedProfile.warnings.length > 0 && (
                  <div className="ui-callout ui-callout--attention">
                    <p>Core reports warnings for {selectedProfile.name}:</p>
                    <ul className="home-warning-list">
                      {selectedProfile.warnings.map((warning) => (
                        <li key={warning}>{warning}</li>
                      ))}
                    </ul>
                  </div>
                )}
                {profileUnavailable && (
                  <p className="ui-callout ui-callout--danger" role="alert">
                    This profile is declared but not executable. Choose an
                    executable profile or the Core default.
                  </p>
                )}

                <p className="ui-callout ui-callout--info home-mode-help">
                  <strong>
                    Every request stops for explicit operator approval.
                  </strong>{" "}
                  Core assesses the request, freezes a plan and waits for your
                  approval before any Campaign starts. This build has no
                  automatic start for intake requests.
                </p>

                {policySetup && (
                  <div
                    className="ui-callout ui-callout--attention home-policy"
                    role="status"
                  >
                    <h3>Enable governed execution</h3>
                    <p>
                      Core requires an explicit, bounded execution policy for
                      this Project and exact profile revision before it can
                      create the Lead request. This grant is non-autonomous and
                      limited to R0 work; it does not enable auto-start.
                    </p>
                    {policySetup.scope.profile && policySetup.scope.project ? (
                      <dl className="request-facts">
                        <div>
                          <dt>Profile</dt>
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
                          <dt>Project</dt>
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
                          <dt>Allowed paths</dt>
                          <dd>
                            {policySetup.scope.project.allowedPaths.join(
                              ", ",
                            ) || "None"}
                          </dd>
                        </div>
                        <div>
                          <dt>Capabilities</dt>
                          <dd>
                            {policySetup.scope.project.capabilities.join(
                              ", ",
                            ) || "None"}
                          </dd>
                        </div>
                        <div>
                          <dt>Implementer route</dt>
                          <dd>
                            {policySetup.scope.profile.implementer.provider}/
                            {policySetup.scope.profile.implementer.model}
                          </dd>
                        </div>
                        <div>
                          <dt>Objective scope</dt>
                          <dd>
                            {policySetup.scope.profile.grantTerms.objectiveClasses.join(
                              ", ",
                            )}{" "}
                            · risk{" "}
                            {policySetup.scope.profile.grantTerms.riskClasses.join(
                              ", ",
                            )}{" "}
                            ·{" "}
                            {policySetup.scope.profile.grantTerms
                              .autonomousStart
                              ? "autonomous start enabled"
                              : "non-autonomous; explicit approval required"}
                          </dd>
                        </div>
                        <div>
                          <dt>Per-request limits</dt>
                          <dd>
                            {
                              policySetup.scope.profile.grantTerms
                                .maxAttemptsPerRequest
                            }{" "}
                            attempts ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxSessionsPerRequest
                            }{" "}
                            sessions ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxTurnsPerRequest
                            }{" "}
                            turns ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxInputTokensPerRequest
                            }{" "}
                            input tokens ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxOutputTokensPerRequest
                            }{" "}
                            output tokens ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxCostMicrosPerRequest
                            }{" "}
                            cost micros ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxTimeoutSecsPerRequest
                            }
                            s timeout ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxWallClockSecsPerRequest
                            }
                            s wall clock
                          </dd>
                        </div>
                        <div>
                          <dt>Cumulative grant limits</dt>
                          <dd>
                            {policySetup.scope.profile.grantTerms.maxRequests}{" "}
                            requests ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxCumulativeSessions
                            }{" "}
                            sessions ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxCumulativeAttempts
                            }{" "}
                            attempts · valid{" "}
                            {policySetup.scope.profile.grantTerms.validityDays}{" "}
                            days ·{" "}
                            {
                              policySetup.scope.profile.grantTerms
                                .maxRepairCycles
                            }{" "}
                            repair cycles ·{" "}
                            {policySetup.scope.profile.grantTerms.maxParallel}{" "}
                            parallel
                          </dd>
                        </div>
                      </dl>
                    ) : (
                      <p className="ui-callout ui-callout--danger">
                        Core required an execution policy but did not return the
                        exact profile and Project scope. Refresh the page and
                        try again.
                      </p>
                    )}
                    <Checkbox
                      label="I authorize this bounded non-autonomous R0 execution policy for the exact Project configuration and profile revision shown."
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
                          ? "Enabling governed execution…"
                          : "Enable governed execution"}
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
                    {busy ? "Submitting request…" : "Submit request"}
                  </Button>
                  <span className="muted">
                    Submitting records operator intent. It does not authorize a
                    Campaign.
                  </span>
                </div>
              </form>
            )}
          </Panel>

          <Panel className="home-recent">
            <div className="section-heading">
              <h2>Recent requests</h2>
              <span className="muted">Bounded Core projection</span>
            </div>
            {recentRequests === null && !recentUnavailable && !recentError && (
              <LoadingState label="Loading requests…" />
            )}
            {recentUnavailable && (
              <p className="ui-callout ui-callout--info" role="status">
                Unified request history is not available from this Core build.
                The composer and existing operational views remain available.
              </p>
            )}
            {recentError && (
              <div className="ui-callout ui-callout--danger" role="alert">
                <p>{recentError}</p>
                <Button size="sm" onClick={() => void loadRecent()}>
                  Retry requests
                </Button>
              </div>
            )}
            {recentRequests !== null && recentRequests.length === 0 && (
              <EmptyState
                title="No requests yet"
                description="Submit the first request above. Its state and conversation appear here."
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
                        {projectName(request.projectId, projects)} ·{" "}
                        {formatTimestamp(request.updatedAt)}
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

        <aside className="home-side" aria-label="Current Factory status">
          <Panel className="home-status">
            <div className="section-heading">
              <h2>Core status</h2>
              {overview && (
                <Badge
                  tone={overview.runtime.core_healthy ? "success" : "danger"}
                >
                  {overview.runtime.core_healthy ? "HEALTHY" : "UNHEALTHY"}
                </Badge>
              )}
            </div>
            {overview === null && overviewError === null && (
              <LoadingState label="Checking Core…" />
            )}
            {overview === null && overviewError !== null && (
              <div className="ui-callout ui-callout--danger" role="alert">
                <p>Core status is unavailable: {overviewError}</p>
                <Button size="sm" onClick={() => void loadOverview()}>
                  Retry status
                </Button>
              </div>
            )}
            {overview && (
              <>
                <p className="muted home-status-time">
                  Snapshot {formatTimestamp(overview.as_of)}. Refreshed every 5
                  seconds from Core.
                </p>
                {overviewError && (
                  <div
                    className="ui-callout ui-callout--attention"
                    role="status"
                  >
                    <p>
                      Latest refresh failed: {overviewError}. Showing the last
                      authoritative snapshot.
                    </p>
                    <Button size="sm" onClick={() => void loadOverview()}>
                      Retry now
                    </Button>
                  </div>
                )}
                <dl className="home-facts">
                  <div>
                    <dt>Workers</dt>
                    <dd>{overview.runtime.workers}</dd>
                  </div>
                  <div>
                    <dt>Unresolved sessions</dt>
                    <dd>{overview.runtime.unresolved_sessions}</dd>
                  </div>
                  <div>
                    <dt>Unresolved model calls</dt>
                    <dd>{overview.runtime.unresolved_model_calls}</dd>
                  </div>
                  <div>
                    <dt>Telemetry export</dt>
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
              <h2>Active work</h2>
              <Link to="/campaigns">Campaigns</Link>
            </div>
            {overview === null ? (
              <p className="muted">Unavailable until Core status loads.</p>
            ) : (
              <>
                <dl className="home-facts">
                  <div>
                    <dt>Authorized campaigns</dt>
                    <dd>{overview.counts.authorized_campaigns}</dd>
                  </div>
                  <div>
                    <dt>Awaiting authorization</dt>
                    <dd>{overview.counts.campaigns_awaiting_authorization}</dd>
                  </div>
                  <div>
                    <dt>Running agents</dt>
                    <dd>{overview.counts.active_agents}</dd>
                  </div>
                  <div>
                    <dt>Blocked signals</dt>
                    <dd>{overview.counts.blocked_signals}</dd>
                  </div>
                </dl>
                <h3 className="home-subheading">Recent events</h3>
                {overview.recent_events.length === 0 ? (
                  <p className="muted">No events recorded.</p>
                ) : (
                  <ul className="home-events">
                    {overview.recent_events.slice(0, 3).map((event) => (
                      <li key={event.sequence}>
                        <code>#{event.sequence}</code>
                        <strong>{event.kind}</strong>
                        <span className="muted">
                          {formatTimestamp(event.recorded_at)}
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
              <h2>Pending decisions</h2>
              <Link to="/inbox">Open Decisions</Link>
            </div>
            {overview === null ? (
              <p className="muted">Unavailable until Core status loads.</p>
            ) : pendingDecisions.length === 0 ? (
              <EmptyState
                title="Nothing needs a decision"
                description="Approvals, Gate repairs and unresolved usage appear when Core records them."
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
