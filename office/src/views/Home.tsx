import {
  Activity,
  ArrowRight,
  ChevronDown,
  FileText,
  FolderGit2,
  History,
  Inbox,
  Send,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Wallet,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Checkbox, Input, Select, Textarea } from "../components/ui/Field";
import { EmptyState, LoadingState, Panel } from "../components/ui/Surface";
import {
  CoreApiError,
  coreApi,
  type ExecutionProfile,
  type IntakeExecutionPolicyScope,
  type IntakeRequestProjection,
  type IntakeRequestSummary,
  type NewIntakeRequest,
  newIdempotencyKey,
  type OfficeOverview,
  type Project,
} from "../coreApi";
import { shortId } from "../format";
import { type MessageKey, useI18n } from "../i18n";
import { HomeThread } from "./HomeThread";
import { IntakeStateBadge, isTerminalIntakeState } from "./RequestState";

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

const SUGGESTIONS: Array<{ label: MessageKey; seed: MessageKey }> = [
  { label: "home.suggestions.issue", seed: "home.suggestions.issueSeed" },
  {
    label: "home.suggestions.performance",
    seed: "home.suggestions.performanceSeed",
  },
  {
    label: "home.suggestions.integration",
    seed: "home.suggestions.integrationSeed",
  },
  { label: "home.suggestions.refactor", seed: "home.suggestions.refactorSeed" },
  { label: "home.suggestions.tests", seed: "home.suggestions.testsSeed" },
];

function latestConversation(
  requests: IntakeRequestSummary[] | null,
): IntakeRequestSummary | null {
  if (requests === null || requests.length === 0) return null;
  return (
    requests.find((request) => !isTerminalIntakeState(request.state)) ??
    requests[0]
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
  const [thread, setThread] = useState<IntakeRequestProjection | null>(null);
  const [threadError, setThreadError] = useState<string | null>(null);
  const [threadLoading, setThreadLoading] = useState(false);
  const threadGeneration = useRef(0);

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

  const threadSummary = useMemo(
    () => latestConversation(recentRequests),
    [recentRequests],
  );
  const threadId = threadSummary?.id ?? null;

  useEffect(() => {
    const generation = ++threadGeneration.current;
    if (threadId === null) {
      setThread(null);
      setThreadError(null);
      setThreadLoading(false);
      return () => {
        if (threadGeneration.current === generation) {
          threadGeneration.current += 1;
        }
      };
    }
    setThread(null);
    setThreadError(null);
    setThreadLoading(true);

    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = async () => {
      try {
        const next = await coreApi.getIntakeRequest(threadId);
        if (!active || threadGeneration.current !== generation) return;
        setThread(next);
        setThreadError(null);
      } catch (cause) {
        if (!active || threadGeneration.current !== generation) return;
        setThreadError(errorMessage(cause, t("home.thread.error")));
      } finally {
        if (active && threadGeneration.current === generation) {
          setThreadLoading(false);
          // Schedule the next poll only after this one settles. This prevents
          // overlapping requests from resolving out of order and replacing a
          // fresher Core projection with an older response.
          timer = setTimeout(() => void refresh(), 8000);
        }
      }
    };
    void refresh();

    return () => {
      active = false;
      if (timer !== undefined) clearTimeout(timer);
      if (threadGeneration.current === generation) {
        threadGeneration.current += 1;
      }
    };
  }, [threadId, t]);

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
  const profileDefaultLabel = selectedProfile
    ? projectId && selectedProfile.projectDefaults.includes(projectId)
      ? t("home.profile.projectDefault")
      : selectedProfile.globalDefault
        ? t("home.profile.globalDefault")
        : null
    : null;
  const pendingDecisions =
    overview?.recent_decisions.filter((decision) => decision.actionable) ?? [];
  const needsReview = overview
    ? overview.runtime.unresolved_sessions +
      overview.runtime.unresolved_model_calls
    : 0;
  const threadProject = threadSummary
    ? projectName(threadSummary.projectId, projects, t)
    : "";

  function resetRequestKey() {
    requestKey.current = newIdempotencyKey();
    setFormError(null);
    setPolicySetup(null);
    setPolicyAcknowledged(false);
    setPolicyError(null);
  }

  function applySuggestion(seed: string) {
    setObjective((current) => {
      if (current.trim() === "") return seed;
      return `${current}${current.endsWith("\n") ? "" : "\n"}${seed}`;
    });
    setFieldErrors((current) => ({ ...current, objective: undefined }));
    resetRequestKey();
    document.getElementById("home-objective")?.focus();
  }

  function handleComposerKeyDown(
    event: React.KeyboardEvent<HTMLTextAreaElement>,
  ) {
    if (event.key !== "Enter" || (!event.metaKey && !event.ctrlKey)) return;
    event.preventDefault();
    event.currentTarget.form?.requestSubmit();
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
    const nextErrors: { project?: string; objective?: string; spend?: string } =
      {};
    const spendMicros = Math.round(Number(spendUsd) * 1_000_000);
    if (projectId === "") nextErrors.project = t("home.validation.project");
    if (trimmed === "") {
      nextErrors.objective = t("home.validation.objective");
    }
    if (
      !Number.isSafeInteger(spendMicros) ||
      spendMicros <= 0 ||
      spendMicros > 9_000_000_000_000
    ) {
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
      <h1 className="ui-visually-hidden">{t("home.title")}</h1>

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
            <div className="home-composer-head">
              <span className="home-composer-icon" aria-hidden="true">
                <Sparkles size={22} />
              </span>
              <div className="home-composer-head-copy">
                <span className="home-kicker">{t("home.lead.kicker")}</span>
                <h2>{t("home.composer.title")}</h2>
                <p>{t("home.composer.description")}</p>
              </div>
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
                <Textarea
                  id="home-objective"
                  label={t("home.objective.label")}
                  labelHidden
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
                  error={fieldErrors.objective}
                  className="home-objective"
                  rows={4}
                  onKeyDown={handleComposerKeyDown}
                  required
                />

                <fieldset className="home-suggestions">
                  <legend className="ui-visually-hidden">
                    {t("home.suggestions.label")}
                  </legend>
                  {SUGGESTIONS.map((suggestion) => (
                    <button
                      key={suggestion.label}
                      type="button"
                      className="home-suggestion"
                      onClick={() => applySuggestion(t(suggestion.seed))}
                    >
                      {t(suggestion.label)}
                    </button>
                  ))}
                </fieldset>

                <div className="home-composer-toolbar">
                  <div className="home-context-chips">
                    <Select
                      id="home-project"
                      label={
                        <>
                          <FolderGit2 size={13} aria-hidden="true" />
                          {t("home.project.label")}
                        </>
                      }
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
                      required
                    >
                      <option value="">{t("home.project.select")}</option>
                      {projects.map((project) => (
                        <option key={project.id} value={project.id}>
                          {project.repository}
                        </option>
                      ))}
                    </Select>

                    <Select
                      id="home-profile"
                      label={
                        <>
                          <SlidersHorizontal size={13} aria-hidden="true" />
                          {t("home.profile.label")}
                        </>
                      }
                      value={profileId}
                      onChange={(event) => {
                        setProfileId(event.target.value);
                        resetRequestKey();
                      }}
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
                        </option>
                      ))}
                    </Select>

                    <Input
                      id="home-spend"
                      className="home-spend-input"
                      label={
                        <>
                          <Wallet size={13} aria-hidden="true" />
                          {t("home.spend.label")}
                        </>
                      }
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
                      error={fieldErrors.spend}
                      required
                    />
                  </div>
                  <Button
                    type="submit"
                    variant="primary"
                    size="lg"
                    className="home-send"
                    busy={busy}
                    disabled={
                      busy || profileUnavailable || policySetup !== null
                    }
                  >
                    {busy ? t("home.submitting") : t("home.submit")}
                    {!busy && <Send size={17} aria-hidden="true" />}
                  </Button>
                </div>

                <p className="home-spend-note">{t("home.spend.help")}</p>

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

                {selectedProfile && (
                  <details className="home-profile-details">
                    <summary>
                      {profileDefaultLabel && (
                        <span className="home-profile-default">
                          {profileDefaultLabel}
                        </span>
                      )}
                      <span>{t("home.profile.routes")}</span>
                      <ChevronDown size={14} aria-hidden="true" />
                    </summary>
                    <dl className="home-profile-routes">
                      {(
                        [
                          ["lead", "execProfiles.role.lead"],
                          ["implementer", "execProfiles.role.implementer"],
                          [
                            "semanticReviewer",
                            "execProfiles.role.semanticReviewer",
                          ],
                        ] as const
                      ).map(([role, labelKey]) => (
                        <div key={role}>
                          <dt>{t(labelKey)}</dt>
                          <dd>
                            <span className="muted">
                              {selectedProfile.roles[role].provider}/
                            </span>
                            <code>{selectedProfile.roles[role].model}</code>
                          </dd>
                        </div>
                      ))}
                    </dl>
                    <p className="home-profile-route-note">
                      {t("home.profile.routesHint")}
                    </p>
                  </details>
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

                <p className="home-approval-note">
                  <ShieldCheck size={16} aria-hidden="true" />
                  <span>
                    <strong>{t("home.approval.title")}</strong>{" "}
                    {t("home.approval.body")}
                  </span>
                </p>

                {policySetup && (
                  <div
                    className="ui-callout ui-callout--attention home-policy"
                    role="status"
                  >
                    <h3>{t("home.policy.title")}</h3>
                    <p>{t("home.policy.description")}</p>
                    {policySetup.scope.profile && policySetup.scope.project ? (
                      <>
                        <dl className="request-facts">
                          <div>
                            <dt>{t("home.policy.profile")}</dt>
                            <dd>
                              {policySetup.scope.profile.name ??
                                t("home.policy.none")}{" "}
                              r{policySetup.scope.profile.revision} ·{" "}
                              <code
                                title={policySetup.scope.profile.contentHash}
                              >
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
                              <code
                                title={policySetup.scope.project.configHash}
                              >
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
                              {policySetup.scope.profile.implementer
                                ? `${policySetup.scope.profile.implementer.provider}/${policySetup.scope.profile.implementer.model}`
                                : t("home.policy.none")}
                            </dd>
                          </div>
                          {policySetup.scope.profile.grantTerms && (
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
                          )}
                          {policySetup.scope.profile.grantTerms && (
                            <div>
                              <dt>{t("home.policy.grantLifetime")}</dt>
                              <dd>
                                {t("home.policy.grantLifetimeValue", {
                                  days: policySetup.scope.profile.grantTerms
                                    .validityDays,
                                  parallel:
                                    policySetup.scope.profile.grantTerms
                                      .maxParallel,
                                })}
                              </dd>
                            </div>
                          )}
                          <div>
                            <dt>{t("home.policy.inferenceEnvelope")}</dt>
                            <dd>
                              {t("home.policy.inferenceEnvelopeValue", {
                                amount: (
                                  (policySetup.pending.authorized_micros ?? 0) /
                                  1_000_000
                                ).toFixed(2),
                              })}
                            </dd>
                          </div>
                        </dl>
                        <p className="muted home-policy-technical">
                          {t("home.policy.technical")}
                        </p>
                      </>
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
                    <div className="home-form-actions">
                      <Button
                        type="button"
                        variant="primary"
                        busy={policyBusy}
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

                <p className="home-keyboard-hint">{t("home.keyboardHint")}</p>
              </form>
            )}
          </Panel>

          <Panel className="home-recent" data-testid="home-recent">
            <div className="home-section-head">
              <div className="home-section-title">
                <span className="home-section-icon" aria-hidden="true">
                  <History size={16} />
                </span>
                <h2>{t("home.recent.title")}</h2>
              </div>
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
                    <span className="home-request-icon" aria-hidden="true">
                      <FileText size={16} />
                    </span>
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
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </Panel>
        </div>

        <aside className="home-side" aria-label={t("home.side.aria")}>
          <HomeThread
            summary={threadSummary}
            projection={thread}
            projectName={threadProject}
            loading={threadLoading}
            error={threadError}
          />

          <Panel className="home-decisions" data-testid="home-decisions">
            <div className="home-section-head">
              <div className="home-section-title">
                <span className="home-section-icon" aria-hidden="true">
                  <Inbox size={16} />
                </span>
                <h2>{t("home.decisions.title")}</h2>
              </div>
              <Link className="home-section-link" to="/inbox">
                {t("home.decisions.open")}
                <ArrowRight size={13} aria-hidden="true" />
              </Link>
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
                    <span
                      className={`home-decision-dot ${
                        decision.severity === "critical"
                          ? "home-decision-dot--danger"
                          : decision.severity === "attention"
                            ? "home-decision-dot--attention"
                            : ""
                      }`}
                      aria-hidden="true"
                    />
                    <div className="home-decision-copy">
                      <Link to={decision.target}>{decision.title}</Link>
                      <p>{decision.detail}</p>
                    </div>
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
                  </li>
                ))}
              </ul>
            )}
          </Panel>

          <Panel className="home-status" data-testid="home-status">
            <div className="home-section-head">
              <div className="home-section-title">
                <span className="home-section-icon" aria-hidden="true">
                  <Activity size={16} />
                </span>
                <h2>{t("home.status.title")}</h2>
              </div>
              {overview && (
                <Badge
                  tone={overview.runtime.core_healthy ? "success" : "danger"}
                >
                  {overview.runtime.core_healthy
                    ? t("home.status.healthy")
                    : t("home.status.unhealthy")}
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
                <p className="home-status-time">
                  {t("home.status.updated", {
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
                    <dt>{t("home.status.activeAgents")}</dt>
                    <dd>{formatNumber(overview.counts.active_agents)}</dd>
                  </div>
                </dl>
                {needsReview > 0 && (
                  <p className="home-status-attention">
                    <span className="home-status-dot" aria-hidden="true" />
                    {t("home.status.needsReview", {
                      count: formatNumber(needsReview),
                    })}
                  </p>
                )}
                <div className="home-status-links">
                  <Link to="/runtime">{t("home.status.openRuntime")}</Link>
                  <Link to="/overview">{t("home.status.openOverview")}</Link>
                </div>
              </>
            )}
          </Panel>
        </aside>
      </div>
    </div>
  );
}
