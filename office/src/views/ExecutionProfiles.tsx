import { useCallback, useEffect, useRef, useState } from "react";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Checkbox, Input, Select, Textarea } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  Panel,
} from "../components/ui/Surface";
import {
  CoreApiError,
  coreApi,
  type ExecutionOperationLimits,
  type ExecutionProfile,
  type ExecutionProfileCategory,
  type ExecutionProfileRole,
  type ExecutionProfileWrite,
  type ExecutionProgressPolicy,
  type ExecutionReasoningEffort,
  type ExecutionRiskClass,
  type ExecutionRoleRoute,
  type Project,
  type ProviderModelDiscovery,
  type ProviderView,
  type ResolvedExecutionProfile,
  type SupportedExecutionRoute,
} from "../coreApi";
import {
  DEFAULT_OPERATION_LIMITS,
  DEFAULT_PROGRESS_POLICY,
} from "../executionDefaults";
import { formatTimestamp } from "../format";
import { type MessageKey, useI18n } from "../i18n";

const ROLES: {
  id: ExecutionProfileRole;
  labelKey: MessageKey;
  responsibilityKey: MessageKey;
}[] = [
  {
    id: "lead",
    labelKey: "execProfiles.role.lead",
    responsibilityKey: "execProfiles.role.leadResponsibility",
  },
  {
    id: "implementer",
    labelKey: "execProfiles.role.implementer",
    responsibilityKey: "execProfiles.role.implementerResponsibility",
  },
  {
    id: "semanticReviewer",
    labelKey: "execProfiles.role.semanticReviewer",
    responsibilityKey: "execProfiles.role.semanticReviewerResponsibility",
  },
];

const CATEGORIES: ExecutionProfileCategory[] = [
  "BUDGET",
  "BALANCED",
  "QUALITY",
  "CUSTOM",
];

const RISK_CLASSES: ExecutionRiskClass[] = ["R0", "R1", "R2", "R3"];

function categoryKey(category: ExecutionProfileCategory): MessageKey {
  return `execProfiles.category.${category}` as MessageKey;
}

const REASONING: ExecutionReasoningEffort[] = ["low", "medium", "high"];

interface RoleDraft {
  provider: string;
  model: string;
  reasoning: "" | ExecutionReasoningEffort;
}

interface ProfileDraft {
  id: string | null;
  expectedRevision: number | null;
  legacyV1: boolean;
  name: string;
  description: string;
  category: ExecutionProfileCategory;
  roles: Record<ExecutionProfileRole, RoleDraft>;
  operations: ExecutionOperationLimits;
  progress: ExecutionProgressPolicy;
  autoStartEnabled: boolean;
  autoStartRiskClasses: ExecutionRiskClass[];
}

function routeDefaults(
  routes: SupportedExecutionRoute[],
  preferred: string,
): { provider: string; model: string } {
  const match =
    routes.find((route) => route.provider === preferred) ?? routes[0];
  return match
    ? { provider: match.provider, model: match.model }
    : { provider: "", model: "" };
}

function newDraft(routes: SupportedExecutionRoute[]): ProfileDraft {
  const defaultRoute = routeDefaults(routes, "opencode-go");
  const role = (route: { provider: string; model: string }): RoleDraft => ({
    provider: route.provider,
    model: route.model,
    reasoning: "medium",
  });
  return {
    id: null,
    expectedRevision: null,
    legacyV1: false,
    name: "Balanced",
    description: "Default supported routes",
    category: "BALANCED",
    roles: {
      lead: role(defaultRoute),
      implementer: role(defaultRoute),
      semanticReviewer: role(defaultRoute),
    },
    operations: { ...DEFAULT_OPERATION_LIMITS },
    progress: { ...DEFAULT_PROGRESS_POLICY },
    autoStartEnabled: false,
    autoStartRiskClasses: [],
  };
}

function draftFromProfile(profile: ExecutionProfile): ProfileDraft {
  const role = (route: ExecutionRoleRoute): RoleDraft => ({
    provider: route.provider,
    model: route.model,
    reasoning: route.reasoning ?? "",
  });
  return {
    id: profile.id,
    expectedRevision: profile.revision,
    legacyV1: profile.schemaVersion === 1,
    name: profile.name,
    description: profile.description,
    category: profile.category,
    roles: {
      lead: role(profile.roles.lead),
      implementer: role(profile.roles.implementer),
      semanticReviewer: role(profile.roles.semanticReviewer),
    },
    operations: { ...(profile.operations ?? DEFAULT_OPERATION_LIMITS) },
    progress: { ...(profile.progress ?? DEFAULT_PROGRESS_POLICY) },
    autoStartEnabled: profile.autoStart.enabled,
    autoStartRiskClasses: [...profile.autoStart.riskClasses],
  };
}

function roleToRoute(role: RoleDraft): ExecutionRoleRoute {
  return {
    provider: role.provider,
    model: role.model,
    ...(role.reasoning === "" ? {} : { reasoning: role.reasoning }),
  };
}

function draftToWrite(draft: ProfileDraft): ExecutionProfileWrite {
  return {
    name: draft.name.trim(),
    description: draft.description.trim(),
    category: draft.category,
    roles: {
      lead: roleToRoute(draft.roles.lead),
      implementer: roleToRoute(draft.roles.implementer),
      semanticReviewer: roleToRoute(draft.roles.semanticReviewer),
    },
    operations: { ...draft.operations },
    progress: { ...draft.progress },
    auto_start: {
      enabled: draft.autoStartEnabled,
      risk_classes: [...draft.autoStartRiskClasses].sort(),
    },
  };
}

type DiscoveryState = Record<
  string,
  ProviderModelDiscovery | "loading" | { error: string }
>;

function isDiscovery(
  value: DiscoveryState[string] | undefined,
): value is ProviderModelDiscovery {
  return (
    typeof value === "object" &&
    value !== null &&
    "status" in value &&
    "models" in value
  );
}

function discoveryError(
  value: DiscoveryState[string] | undefined,
): string | null {
  if (typeof value === "object" && value !== null && "error" in value) {
    return value.error;
  }
  return null;
}

function uniqueModels(models: string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const model of models) {
    if (model !== "" && !seen.has(model)) {
      seen.add(model);
      result.push(model);
    }
  }
  return result;
}

function validateModelId(
  model: string,
  t: ReturnType<typeof useI18n>["t"],
): string | null {
  if (model.trim() === "") return t("execProfiles.validation.modelRequired");
  if (model.length > 128) return t("execProfiles.validation.modelTooLong");
  if (model.trim() !== model || /\s/.test(model)) {
    return t("execProfiles.validation.modelWhitespace");
  }
  return null;
}

function routeSupportWarning(
  role: { id: ExecutionProfileRole; labelKey: MessageKey },
  provider: ProviderView | undefined,
  model: string,
  discoveredModels: string[],
  baselineModels: string[],
  t: ReturnType<typeof useI18n>["t"],
): string | null {
  const label = t(role.labelKey);
  if (provider === undefined) {
    return t("execProfiles.warning.unknownProvider", { role: label });
  }
  if (!provider.configured) {
    return t("execProfiles.warning.unconfiguredProvider", {
      role: label,
      provider: provider.provider,
    });
  }
  if (!provider.enabled) {
    return t("execProfiles.warning.disabledProvider", {
      role: label,
      provider: provider.provider,
    });
  }
  if (role.id !== "implementer" && provider.kind !== "opencode") {
    return t("execProfiles.warning.appServerProvider", {
      role: label,
      kind: provider.kind,
    });
  }
  const advertised = uniqueModels([
    provider.model,
    ...baselineModels,
    ...discoveredModels,
  ]);
  if (model !== "" && !advertised.includes(model)) {
    return t("execProfiles.warning.unadvertisedModel", {
      role: label,
      model,
      provider: provider.provider,
    });
  }
  return null;
}

function validateDraft(
  draft: ProfileDraft,
  routes: SupportedExecutionRoute[],
  providers: ProviderView[],
  t: ReturnType<typeof useI18n>["t"],
): string | null {
  const name = draft.name.trim();
  if (name.length === 0) return t("execProfiles.validation.nameRequired");
  if (name.length > 80) return t("execProfiles.validation.nameTooLong");
  if (draft.description.trim().length > 500) {
    return t("execProfiles.validation.descriptionTooLong");
  }
  if (draft.autoStartEnabled && draft.autoStartRiskClasses.length === 0) {
    return t("execProfiles.validation.riskRequired");
  }
  const providerIds =
    providers.length > 0
      ? providers.map((provider) => provider.provider)
      : uniqueModels(routes.map((route) => route.provider));
  for (const role of ROLES) {
    const value = draft.roles[role.id];
    if (!providerIds.includes(value.provider)) {
      return t("execProfiles.validation.providerRequired", {
        role: t(role.labelKey),
      });
    }
    const modelError = validateModelId(value.model, t);
    if (modelError) {
      return `${t(role.labelKey)}: ${modelError}`;
    }
  }
  const operationChecks: Array<[MessageKey, number, number, number]> = [
    [
      "execProfiles.operations.modelRequestTimeout",
      draft.operations.modelRequestTimeoutSecs,
      1,
      3_600,
    ],
    [
      "execProfiles.operations.toolCallTimeout",
      draft.operations.toolCallTimeoutSecs,
      1,
      3_600,
    ],
    [
      "execProfiles.operations.maxRequestBytes",
      draft.operations.maxRequestBytes,
      1,
      64 * 1024 * 1024,
    ],
    [
      "execProfiles.operations.maxResponseBytes",
      draft.operations.maxResponseBytes,
      1,
      64 * 1024 * 1024,
    ],
    [
      "execProfiles.operations.maxGenerationTokens",
      draft.operations.maxGenerationTokens,
      1,
      1_000_000,
    ],
    [
      "execProfiles.operations.maxInFlight",
      draft.operations.maxInFlightOperations,
      1,
      32,
    ],
  ];
  for (const [label, value, min, max] of operationChecks) {
    if (!Number.isFinite(value) || value < min || value > max) {
      return t("execProfiles.validation.operationRange", {
        field: t(label),
        min,
        max,
      });
    }
  }
  const progressChecks: Array<[MessageKey, number, number, number]> = [
    [
      "execProfiles.progress.diagnoseAfter",
      draft.progress.diagnoseAfterStagnantCheckpoints,
      1,
      100,
    ],
    [
      "execProfiles.progress.escalateAfter",
      draft.progress.escalateAfterUnsuccessfulReplans,
      1,
      100,
    ],
    [
      "execProfiles.progress.observationTimeout",
      draft.progress.observationTimeoutSecs,
      1,
      3_600,
    ],
  ];
  for (const [label, value, min, max] of progressChecks) {
    if (!Number.isFinite(value) || value < min || value > max) {
      return t("execProfiles.validation.progressRange", {
        field: t(label),
        min,
        max,
      });
    }
  }
  return null;
}

function errorMessage(cause: unknown, fallback: string): string {
  if (cause instanceof CoreApiError) return cause.message;
  if (cause instanceof Error && cause.message !== "") return cause.message;
  return fallback;
}

export function ExecutionProfiles() {
  const { t } = useI18n();
  const [profiles, setProfiles] = useState<ExecutionProfile[]>([]);
  const [routes, setRoutes] = useState<SupportedExecutionRoute[]>([]);
  const [providers, setProviders] = useState<ProviderView[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [discovery, setDiscovery] = useState<DiscoveryState>({});
  const [modelFilters, setModelFilters] = useState<
    Record<ExecutionProfileRole, string>
  >({ lead: "", implementer: "", semanticReviewer: "" });
  const [manualModel, setManualModel] = useState<
    Record<ExecutionProfileRole, boolean>
  >({ lead: false, implementer: false, semanticReviewer: false });
  const requestedDiscovery = useRef(new Set<string>());
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<ProfileDraft | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [projectDefaultTarget, setProjectDefaultTarget] = useState("");
  const [resolution, setResolution] = useState<ResolvedExecutionProfile | null>(
    null,
  );
  const [resolutionError, setResolutionError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [list, projectList, providerList] = await Promise.all([
        coreApi.listExecutionProfiles(),
        coreApi.listProjects(),
        coreApi.listProviders(),
      ]);
      setProfiles(list.profiles);
      setRoutes(list.supportedRoutes);
      setProviders(providerList.providers);
      setProjects(projectList);
      setLastLoadedAt(new Date().toISOString());
      setLoaded(true);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause, t("execProfiles.error.load")));
    }
  }, [t]);

  const discoverProvider = useCallback(
    async (provider: string) => {
      if (provider === "" || requestedDiscovery.current.has(provider)) return;
      requestedDiscovery.current.add(provider);
      setDiscovery((current) => ({ ...current, [provider]: "loading" }));
      try {
        const result = await coreApi.listProviderModels(provider);
        setDiscovery((current) => ({ ...current, [provider]: result }));
      } catch (cause) {
        requestedDiscovery.current.delete(provider);
        setDiscovery((current) => ({
          ...current,
          [provider]: {
            error: errorMessage(cause, t("execProfiles.error.discovery")),
          },
        }));
      }
    },
    [t],
  );

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (
      projectDefaultTarget === "" ||
      !projects.some((project) => project.id === projectDefaultTarget)
    ) {
      const first = projects[0];
      if (first) setProjectDefaultTarget(first.id);
    }
  }, [projectDefaultTarget, projects]);

  const refreshResolution = useCallback(async () => {
    if (projectDefaultTarget === "") {
      setResolution(null);
      setResolutionError(null);
      return;
    }
    try {
      const value =
        await coreApi.getProjectExecutionProfile(projectDefaultTarget);
      setResolution(value);
      setResolutionError(null);
    } catch (cause) {
      if (cause instanceof CoreApiError && cause.status === 404) {
        setResolution(null);
        setResolutionError(null);
        return;
      }
      setResolution(null);
      setResolutionError(
        errorMessage(cause, t("execProfiles.error.resolution")),
      );
    }
  }, [projectDefaultTarget, t]);

  useEffect(() => {
    void refreshResolution();
  }, [refreshResolution]);

  useEffect(() => {
    if (!draft) return;
    for (const role of ROLES) {
      const provider = draft.roles[role.id].provider;
      if (provider !== "") void discoverProvider(provider);
    }
  }, [draft, discoverProvider]);

  function baselineModels(provider: string): string[] {
    return routes
      .filter((route) => route.provider === provider)
      .map((route) => route.model);
  }

  function discoveredModels(provider: string): string[] {
    const entry = discovery[provider];
    return isDiscovery(entry) && entry.status === "AVAILABLE"
      ? entry.models
      : [];
  }

  function modelOptions(provider: string, current: string): string[] {
    const configured = providers.find(
      (candidate) => candidate.provider === provider,
    );
    return uniqueModels([
      ...(configured?.model ? [configured.model] : []),
      ...baselineModels(provider),
      ...discoveredModels(provider),
      ...(current !== "" ? [current] : []),
    ]);
  }

  function providerOptions(): { value: string; label: string }[] {
    if (providers.length > 0) {
      return providers.map((provider) => ({
        value: provider.provider,
        label: provider.displayName,
      }));
    }
    return uniqueModels(routes.map((route) => route.provider)).map(
      (provider) => ({ value: provider, label: provider }),
    );
  }

  function changeRoleProvider(role: ExecutionProfileRole, provider: string) {
    const nextModel = modelOptions(provider, "")[0] ?? "";
    setManualModel((current) => ({ ...current, [role]: false }));
    setModelFilters((current) => ({ ...current, [role]: "" }));
    updateRole(role, { provider, model: nextModel });
    void discoverProvider(provider);
  }

  function draftWarnings(): string[] {
    if (!draft) return [];
    const warnings: string[] = [];
    for (const role of ROLES) {
      const value = draft.roles[role.id];
      const provider = providers.find(
        (candidate) => candidate.provider === value.provider,
      );
      const warning = routeSupportWarning(
        role,
        provider,
        value.model,
        discoveredModels(value.provider),
        baselineModels(value.provider),
        t,
      );
      if (warning) warnings.push(warning);
    }
    return warnings;
  }

  function updateRole(role: ExecutionProfileRole, patch: Partial<RoleDraft>) {
    setDraft((current) => {
      if (!current) return current;
      return {
        ...current,
        roles: {
          ...current.roles,
          [role]: { ...current.roles[role], ...patch },
        },
      };
    });
  }

  function updateOperations(patch: Partial<ExecutionOperationLimits>) {
    setDraft((current) =>
      current
        ? { ...current, operations: { ...current.operations, ...patch } }
        : current,
    );
  }

  function updateProgress(patch: Partial<ExecutionProgressPolicy>) {
    setDraft((current) =>
      current
        ? { ...current, progress: { ...current.progress, ...patch } }
        : current,
    );
  }

  function resetRoleInputs() {
    setModelFilters({ lead: "", implementer: "", semanticReviewer: "" });
    setManualModel({
      lead: false,
      implementer: false,
      semanticReviewer: false,
    });
  }

  function selectProfile(profile: ExecutionProfile) {
    setSelectedId(profile.id);
    setDraft(draftFromProfile(profile));
    setFormError(null);
    setStale(false);
    setNotice(null);
    resetRoleInputs();
  }

  function startNewProfile() {
    setSelectedId(null);
    setDraft(newDraft(routes));
    setFormError(null);
    setStale(false);
    setNotice(null);
    resetRoleInputs();
  }

  async function reloadCurrentRevision() {
    if (!draft?.id) return;
    setBusy(true);
    setError(null);
    try {
      const fresh = await coreApi.getExecutionProfile(draft.id);
      setDraft(draftFromProfile(fresh));
      setStale(false);
      setNotice(
        t("execProfiles.notice.reloaded", { revision: fresh.revision }),
      );
      await load();
    } catch (cause) {
      setError(errorMessage(cause, t("execProfiles.error.reload")));
    } finally {
      setBusy(false);
    }
  }

  async function saveProfile(event: React.FormEvent) {
    event.preventDefault();
    if (!draft) return;
    setFormError(null);
    setNotice(null);
    setError(null);
    const validation = validateDraft(draft, routes, providers, t);
    if (validation) {
      setFormError(validation);
      return;
    }
    setBusy(true);
    try {
      const payload = draftToWrite(draft);
      const saved =
        draft.id === null
          ? await coreApi.createExecutionProfile(payload)
          : await coreApi.updateExecutionProfile(draft.id, {
              ...payload,
              expected_revision: draft.expectedRevision ?? 0,
            });
      setSelectedId(saved.id);
      setDraft(draftFromProfile(saved));
      setStale(false);
      setNotice(
        t("execProfiles.notice.saved", {
          name: saved.name,
          revision: saved.revision,
          state: saved.executable
            ? t("execProfiles.state.executable")
            : t("execProfiles.state.draftNotExecutable"),
        }),
      );
      await load();
    } catch (cause) {
      if (
        cause instanceof CoreApiError &&
        cause.status === 409 &&
        cause.message.includes("stale")
      ) {
        setStale(true);
        setFormError(null);
      } else {
        setFormError(errorMessage(cause, t("execProfiles.error.save")));
      }
    } finally {
      setBusy(false);
    }
  }

  async function setDefault(
    profile: ExecutionProfile,
    scope: "global" | "project",
  ) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await coreApi.setExecutionProfileDefault(
        profile.id,
        scope === "global"
          ? { scope: "global" }
          : { scope: "project", project_id: projectDefaultTarget },
      );
      setNotice(
        scope === "global"
          ? t("execProfiles.notice.globalDefaultSet", { name: profile.name })
          : t("execProfiles.notice.projectDefaultSet", { name: profile.name }),
      );
      await load();
      await refreshResolution();
    } catch (cause) {
      setError(errorMessage(cause, t("execProfiles.error.defaultSet")));
    } finally {
      setBusy(false);
    }
  }

  async function clearDefault(
    profile: ExecutionProfile,
    scope: "global" | "project",
  ) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await coreApi.clearExecutionProfileDefault(
        profile.id,
        scope === "global"
          ? { scope: "global" }
          : { scope: "project", project_id: projectDefaultTarget },
      );
      setNotice(
        scope === "global"
          ? t("execProfiles.notice.globalDefaultCleared")
          : t("execProfiles.notice.projectDefaultCleared"),
      );
      await load();
      await refreshResolution();
    } catch (cause) {
      setError(errorMessage(cause, t("execProfiles.error.defaultCleared")));
    } finally {
      setBusy(false);
    }
  }

  if (!loaded) {
    if (error) {
      return (
        <ErrorState
          title={t("execProfiles.unavailableTitle")}
          message={error}
          action={
            <Button onClick={() => void load()}>{t("common.retry")}</Button>
          }
        />
      );
    }
    return <LoadingState label={t("execProfiles.loading")} />;
  }

  const selectedProfile =
    selectedId === null
      ? null
      : (profiles.find((profile) => profile.id === selectedId) ?? null);
  const defaultProject = projects.find(
    (project) => project.id === projectDefaultTarget,
  );

  return (
    <div className="execution-profiles">
      <Panel className="execution-profiles-list">
        <div className="section-heading">
          <div>
            <h2>{t("execProfiles.title", { count: profiles.length })}</h2>
            <p className="muted">{t("execProfiles.description")}</p>
          </div>
          <div className="row">
            <Button
              variant="primary"
              size="sm"
              disabled={busy || routes.length === 0}
              onClick={startNewProfile}
            >
              {t("execProfiles.newProfile")}
            </Button>
            <Button size="sm" disabled={busy} onClick={() => void load()}>
              {t("execProfiles.refresh")}
            </Button>
          </div>
        </div>

        <p className="muted">
          {t("execProfiles.meta", {
            date: lastLoadedAt ? formatTimestamp(lastLoadedAt) : "—",
            routes: routes.length,
            providers: providers.length,
          })}
        </p>

        {error && (
          <div className="ui-callout ui-callout--danger" role="alert">
            <p>{t("execProfiles.error.stale", { error })}</p>
            <Button size="sm" onClick={() => void load()}>
              {t("common.retry")}
            </Button>
          </div>
        )}
        {notice && <p className="notice">{notice}</p>}

        {profiles.length === 0 ? (
          <EmptyState
            title={t("execProfiles.emptyTitle")}
            description={t("execProfiles.emptyDescription")}
            action={
              <Button
                variant="primary"
                disabled={busy || routes.length === 0}
                onClick={startNewProfile}
              >
                {t("execProfiles.emptyAction")}
              </Button>
            }
          />
        ) : (
          <div className="execution-profiles-table">
            <table>
              <thead>
                <tr>
                  <th scope="col">{t("execProfiles.table.profile")}</th>
                  <th scope="col">{t("execProfiles.table.category")}</th>
                  <th scope="col">{t("execProfiles.table.revision")}</th>
                  <th scope="col">{t("execProfiles.table.hash")}</th>
                  <th scope="col">{t("execProfiles.table.executable")}</th>
                  <th scope="col">{t("execProfiles.table.defaults")}</th>
                  <th scope="col">{t("execProfiles.table.action")}</th>
                </tr>
              </thead>
              <tbody>
                {profiles.map((profile) => (
                  <tr key={profile.id}>
                    <th scope="row">
                      <strong>{profile.name}</strong>
                      {profile.description !== "" && (
                        <span className="execution-profile-role-note">
                          {profile.description}
                        </span>
                      )}
                      <span className="execution-profile-routes">
                        {ROLES.map((role) => (
                          <span key={role.id}>
                            {t(role.labelKey)}:{" "}
                            <code>
                              {profile.roles[role.id].provider}/
                              {profile.roles[role.id].model}
                            </code>
                          </span>
                        ))}
                      </span>
                    </th>
                    <td data-label={t("execProfiles.table.category")}>
                      <Badge tone="neutral">
                        {t(categoryKey(profile.category))}
                      </Badge>
                    </td>
                    <td data-label={t("execProfiles.table.revision")}>
                      {profile.revision}
                      <span className="execution-profile-role-note">
                        <code>{profile.revisionId}</code>
                      </span>
                    </td>
                    <td data-label={t("execProfiles.table.hash")}>
                      <code className="execution-profile-hash">
                        {profile.contentHash}
                      </code>
                    </td>
                    <td data-label={t("execProfiles.table.executable")}>
                      <Badge
                        tone={profile.executable ? "success" : "attention"}
                      >
                        {profile.executable ? "EXECUTABLE" : "DRAFT"}
                      </Badge>
                      {profile.warnings.length > 0 && (
                        <span className="execution-profile-role-note">
                          {t("execProfiles.table.warnings", {
                            count: profile.warnings.length,
                          })}
                        </span>
                      )}
                    </td>
                    <td data-label={t("execProfiles.table.defaults")}>
                      {profile.globalDefault && (
                        <Badge tone="info">
                          {t("execProfiles.table.globalDefault")}
                        </Badge>
                      )}
                      {profile.projectDefaults.map((projectId) => (
                        <Badge key={projectId} tone="neutral">
                          {t("execProfiles.table.projectDefault", {
                            id:
                              projects.find(
                                (project) => project.id === projectId,
                              )?.repository ?? projectId.slice(0, 8),
                          })}
                        </Badge>
                      ))}
                      {!profile.globalDefault &&
                        profile.projectDefaults.length === 0 && (
                          <span className="muted">
                            {t("execProfiles.table.none")}
                          </span>
                        )}
                    </td>
                    <td className="execution-profile-row-action">
                      <Button size="sm" onClick={() => selectProfile(profile)}>
                        {t("execProfiles.table.edit")}
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {selectedProfile && (
          <section
            className="execution-profile-defaults"
            aria-label={t("execProfiles.defaults.aria")}
          >
            <h3>{t("execProfiles.defaults.title")}</h3>
            <div className="execution-profile-meta">
              <Badge
                tone={selectedProfile.executable ? "success" : "attention"}
              >
                {selectedProfile.executable ? "EXECUTABLE" : "DRAFT"}
              </Badge>
              <span>
                {t("execProfiles.defaults.revision")}{" "}
                <strong>{selectedProfile.revision}</strong>
              </span>
              <span>
                {t("execProfiles.defaults.revisionId")}{" "}
                <code>{selectedProfile.revisionId}</code>
              </span>
              <span>
                {t("execProfiles.defaults.contentHash")}{" "}
                <code className="execution-profile-hash">
                  {selectedProfile.contentHash}
                </code>
              </span>
            </div>
            {selectedProfile.warnings.length > 0 && (
              <div className="ui-callout ui-callout--attention" role="status">
                <p>{t("execProfiles.defaults.warning")}</p>
                <ul>
                  {selectedProfile.warnings.map((warning) => (
                    <li key={warning}>{warning}</li>
                  ))}
                </ul>
              </div>
            )}
            <ul className="execution-profile-routes">
              {ROLES.map((role) => {
                const route = selectedProfile.roles[role.id];
                return (
                  <li key={role.id}>
                    <strong>{t(role.labelKey)}</strong>{" "}
                    <code>
                      {route.provider}/{route.model}
                    </code>
                    {route.reasoning ? ` · ${route.reasoning}` : ""}
                  </li>
                );
              })}
            </ul>
            <div className="execution-profile-default-actions">
              <div className="row">
                {selectedProfile.globalDefault ? (
                  <Button
                    disabled={busy}
                    onClick={() => {
                      if (selectedProfile)
                        void clearDefault(selectedProfile, "global");
                    }}
                  >
                    {t("execProfiles.defaults.clearGlobal")}
                  </Button>
                ) : (
                  <Button
                    disabled={busy || !selectedProfile.executable}
                    onClick={() => {
                      if (selectedProfile)
                        void setDefault(selectedProfile, "global");
                    }}
                  >
                    {t("execProfiles.defaults.setGlobal")}
                  </Button>
                )}
              </div>
              <div className="execution-profile-project-default">
                <Select
                  label={t("execProfiles.defaults.project")}
                  value={projectDefaultTarget}
                  onChange={(event) =>
                    setProjectDefaultTarget(event.target.value)
                  }
                  disabled={projects.length === 0}
                  help={t("execProfiles.defaults.projectHelp")}
                >
                  {projects.length === 0 && (
                    <option value="">
                      {t("execProfiles.defaults.noProjects")}
                    </option>
                  )}
                  {projects.map((project) => (
                    <option key={project.id} value={project.id}>
                      {project.repository} ({project.id.slice(0, 8)})
                    </option>
                  ))}
                </Select>
                {projectDefaultTarget !== "" &&
                  (selectedProfile.projectDefaults.includes(
                    projectDefaultTarget,
                  ) ? (
                    <Button
                      disabled={busy}
                      onClick={() => {
                        if (selectedProfile)
                          void clearDefault(selectedProfile, "project");
                      }}
                    >
                      {t("execProfiles.defaults.clearProject")}
                    </Button>
                  ) : (
                    <Button
                      disabled={busy || !selectedProfile.executable}
                      onClick={() => {
                        if (selectedProfile)
                          void setDefault(selectedProfile, "project");
                      }}
                    >
                      {t("execProfiles.defaults.setProject")}
                    </Button>
                  ))}
              </div>
            </div>
            <div className="execution-profile-resolution" role="status">
              {projectDefaultTarget === "" ? (
                <p className="muted">
                  {t("execProfiles.defaults.registerProject")}
                </p>
              ) : resolutionError ? (
                <p className="error">
                  {t("execProfiles.defaults.resolutionError", {
                    error: resolutionError,
                  })}
                </p>
              ) : resolution ? (
                <p>
                  {t("execProfiles.defaults.effective", {
                    project: defaultProject?.repository ?? projectDefaultTarget,
                  })}{" "}
                  <code>{resolution.profileId}</code>{" "}
                  {t("execProfiles.defaults.effectiveRevision", {
                    revision: resolution.revision,
                  })}{" "}
                  <Badge tone="info">{resolution.source}</Badge>
                </p>
              ) : (
                <p className="muted">{t("execProfiles.defaults.noDefault")}</p>
              )}
            </div>
          </section>
        )}
      </Panel>

      {draft && (
        <Panel className="execution-profile-editor">
          <form
            className="settings-form"
            onSubmit={(event) => void saveProfile(event)}
          >
            <div className="section-heading">
              <div>
                <h2>
                  {draft.id
                    ? t("execProfiles.editor.editTitle", {
                        name: draft.name,
                      })
                    : t("execProfiles.editor.newTitle")}
                </h2>
                <p className="muted">{t("execProfiles.editor.description")}</p>
              </div>
              {draft.id && (
                <span className="muted">
                  {t("execProfiles.editor.editingRevision", {
                    revision: draft.expectedRevision ?? 0,
                  })}
                </span>
              )}
            </div>

            {stale && (
              <div className="ui-callout ui-callout--danger" role="alert">
                <p>{t("execProfiles.editor.stale")}</p>
                <Button
                  size="sm"
                  disabled={busy}
                  onClick={() => void reloadCurrentRevision()}
                >
                  {t("execProfiles.editor.reload")}
                </Button>
              </div>
            )}
            {formError && (
              <p className="error" role="alert">
                {formError}
              </p>
            )}
            {draftWarnings().length > 0 && (
              <div className="ui-callout ui-callout--attention" role="status">
                <p>{t("execProfiles.editor.draftWarning")}</p>
                <ul>
                  {draftWarnings().map((warning) => (
                    <li key={warning}>{warning}</li>
                  ))}
                </ul>
              </div>
            )}
            {draft.legacyV1 && (
              <p className="ui-callout ui-callout--info" role="status">
                {t("execProfiles.editor.legacyV1")}
              </p>
            )}

            <div className="grid-2">
              <Input
                label={t("execProfiles.editor.name")}
                value={draft.name}
                maxLength={80}
                onChange={(event) =>
                  setDraft((current) =>
                    current
                      ? { ...current, name: event.target.value }
                      : current,
                  )
                }
              />
              <Select
                label={t("execProfiles.editor.category")}
                value={draft.category}
                onChange={(event) =>
                  setDraft((current) =>
                    current
                      ? {
                          ...current,
                          category: event.target
                            .value as ExecutionProfileCategory,
                        }
                      : current,
                  )
                }
              >
                {CATEGORIES.map((category) => (
                  <option key={category} value={category}>
                    {t(categoryKey(category))}
                  </option>
                ))}
              </Select>
            </div>
            <Textarea
              label={t("execProfiles.editor.profileDescription")}
              value={draft.description}
              maxLength={500}
              rows={2}
              onChange={(event) =>
                setDraft((current) =>
                  current
                    ? { ...current, description: event.target.value }
                    : current,
                )
              }
            />

            <section aria-label={t("execProfiles.matrix.aria")}>
              <div className="section-heading">
                <div>
                  <h3>{t("execProfiles.matrix.title")}</h3>
                  <span className="muted">{t("execProfiles.matrix.hint")}</span>
                </div>
                <Button
                  type="button"
                  size="sm"
                  disabled={busy}
                  onClick={() => {
                    requestedDiscovery.current.clear();
                    for (const role of ROLES) {
                      void discoverProvider(draft.roles[role.id].provider);
                    }
                  }}
                >
                  {t("execProfiles.matrix.refresh")}
                </Button>
              </div>
              <div className="execution-profile-matrix">
                <table>
                  <thead>
                    <tr>
                      <th scope="col">{t("execProfiles.matrix.role")}</th>
                      <th scope="col">{t("execProfiles.matrix.provider")}</th>
                      <th scope="col">{t("execProfiles.matrix.model")}</th>
                      <th scope="col">{t("execProfiles.matrix.reasoning")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {ROLES.map((role) => {
                      const value = draft.roles[role.id];
                      const provider = providers.find(
                        (candidate) => candidate.provider === value.provider,
                      );
                      const entry = discovery[value.provider];
                      const error = discoveryError(entry);
                      const filter = modelFilters[role.id].toLowerCase();
                      const options = modelOptions(
                        value.provider,
                        value.model,
                      ).sort((left, right) => left.localeCompare(right));
                      const filtered =
                        filter === ""
                          ? options
                          : options.filter((model) =>
                              model.toLowerCase().includes(filter),
                            );
                      const manual = manualModel[role.id];
                      const configured = provider?.configured === true;
                      return (
                        <tr key={role.id}>
                          <th scope="row">
                            <strong>{t(role.labelKey)}</strong>
                            <span className="execution-profile-role-note">
                              {t(role.responsibilityKey)}
                            </span>
                          </th>
                          <td>
                            <Select
                              label={t("execProfiles.matrix.providerLabel", {
                                role: t(role.labelKey),
                              })}
                              value={value.provider}
                              onChange={(event) =>
                                changeRoleProvider(role.id, event.target.value)
                              }
                            >
                              {providerOptions().map((option) => (
                                <option key={option.value} value={option.value}>
                                  {option.label}
                                </option>
                              ))}
                              {!providerOptions().some(
                                (option) => option.value === value.provider,
                              ) && (
                                <option value={value.provider}>
                                  {t("execProfiles.matrix.notConfigured", {
                                    provider: value.provider,
                                  })}
                                </option>
                              )}
                            </Select>
                            <span className="execution-profile-role-note">
                              {entry === "loading"
                                ? t("execProfiles.matrix.discovering")
                                : error
                                  ? t(
                                      "execProfiles.matrix.catalogUnavailable",
                                      {
                                        error,
                                      },
                                    )
                                  : isDiscovery(entry)
                                    ? entry.status === "AVAILABLE"
                                      ? t("execProfiles.matrix.discovered", {
                                          count: entry.models.length,
                                        })
                                      : t(
                                          "execProfiles.matrix.discoveryUnavailable",
                                          { detail: entry.detail },
                                        )
                                    : configured
                                      ? t(
                                          "execProfiles.matrix.catalogNotLoaded",
                                        )
                                      : t(
                                          "execProfiles.matrix.providerUnconfigured",
                                        )}
                            </span>
                          </td>
                          <td>
                            {!manual && (
                              <>
                                <Input
                                  label={t("execProfiles.matrix.searchModels", {
                                    role: t(role.labelKey),
                                  })}
                                  value={modelFilters[role.id]}
                                  placeholder={t(
                                    "execProfiles.matrix.filterPlaceholder",
                                  )}
                                  onChange={(event) =>
                                    setModelFilters((current) => ({
                                      ...current,
                                      [role.id]: event.target.value,
                                    }))
                                  }
                                />
                                <Select
                                  label={t("execProfiles.matrix.modelLabel", {
                                    role: t(role.labelKey),
                                  })}
                                  value={value.model}
                                  onChange={(event) =>
                                    updateRole(role.id, {
                                      model: event.target.value,
                                    })
                                  }
                                >
                                  {filtered.length === 0 && (
                                    <option value={value.model}>
                                      {value.model === ""
                                        ? t(
                                            "execProfiles.matrix.noAdvertisedModel",
                                          )
                                        : t("execProfiles.matrix.unverified", {
                                            model: value.model,
                                          })}
                                    </option>
                                  )}
                                  {filtered.map((model) => (
                                    <option key={model} value={model}>
                                      {model}
                                    </option>
                                  ))}
                                </Select>
                              </>
                            )}
                            {manual && (
                              <Input
                                label={t("execProfiles.matrix.manualLabel", {
                                  role: t(role.labelKey),
                                })}
                                value={value.model}
                                placeholder="provider-model-id"
                                help={t("execProfiles.matrix.manualHelp")}
                                onChange={(event) =>
                                  updateRole(role.id, {
                                    model: event.target.value,
                                  })
                                }
                              />
                            )}
                            <Checkbox
                              label={t("execProfiles.matrix.manualToggle", {
                                role: t(role.labelKey),
                              })}
                              checked={manual}
                              onChange={(event) =>
                                setManualModel((current) => ({
                                  ...current,
                                  [role.id]: event.target.checked,
                                }))
                              }
                            />
                          </td>
                          <td>
                            <Select
                              label={t("execProfiles.matrix.reasoningLabel", {
                                role: t(role.labelKey),
                              })}
                              value={value.reasoning}
                              onChange={(event) =>
                                updateRole(role.id, {
                                  reasoning: event.target
                                    .value as RoleDraft["reasoning"],
                                })
                              }
                            >
                              <option value="">
                                {t("execProfiles.matrix.providerDefault")}
                              </option>
                              {REASONING.map((reasoning) => (
                                <option key={reasoning} value={reasoning}>
                                  {reasoning}
                                </option>
                              ))}
                            </Select>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </section>

            <details className="settings-advanced execution-profile-advanced">
              <summary>{t("execProfiles.advanced.title")}</summary>
              <p className="muted">{t("execProfiles.advanced.hint")}</p>
              <div className="grid-2">
                <Input
                  label={t("execProfiles.operations.modelRequestTimeout")}
                  type="number"
                  min={1}
                  max={3_600}
                  value={draft.operations.modelRequestTimeoutSecs}
                  help={t("execProfiles.operations.modelRequestTimeoutHelp")}
                  onChange={(event) =>
                    updateOperations({
                      modelRequestTimeoutSecs: Number(event.target.value),
                    })
                  }
                />
                <Input
                  label={t("execProfiles.operations.toolCallTimeout")}
                  type="number"
                  min={1}
                  max={3_600}
                  value={draft.operations.toolCallTimeoutSecs}
                  onChange={(event) =>
                    updateOperations({
                      toolCallTimeoutSecs: Number(event.target.value),
                    })
                  }
                />
                <Input
                  label={t("execProfiles.operations.maxRequestBytes")}
                  type="number"
                  min={1}
                  max={64 * 1024 * 1024}
                  value={draft.operations.maxRequestBytes}
                  onChange={(event) =>
                    updateOperations({
                      maxRequestBytes: Number(event.target.value),
                    })
                  }
                />
                <Input
                  label={t("execProfiles.operations.maxResponseBytes")}
                  type="number"
                  min={1}
                  max={64 * 1024 * 1024}
                  value={draft.operations.maxResponseBytes}
                  onChange={(event) =>
                    updateOperations({
                      maxResponseBytes: Number(event.target.value),
                    })
                  }
                />
                <Input
                  label={t("execProfiles.operations.maxGenerationTokens")}
                  type="number"
                  min={1}
                  max={1_000_000}
                  value={draft.operations.maxGenerationTokens}
                  onChange={(event) =>
                    updateOperations({
                      maxGenerationTokens: Number(event.target.value),
                    })
                  }
                />
                <Input
                  label={t("execProfiles.operations.maxInFlight")}
                  type="number"
                  min={1}
                  max={32}
                  value={draft.operations.maxInFlightOperations}
                  onChange={(event) =>
                    updateOperations({
                      maxInFlightOperations: Number(event.target.value),
                    })
                  }
                />
              </div>
              <h4>{t("execProfiles.progress.title")}</h4>
              <p className="muted">{t("execProfiles.progress.hint")}</p>
              <div className="grid-2">
                <Input
                  label={t("execProfiles.progress.diagnoseAfter")}
                  type="number"
                  min={1}
                  max={100}
                  value={draft.progress.diagnoseAfterStagnantCheckpoints}
                  onChange={(event) =>
                    updateProgress({
                      diagnoseAfterStagnantCheckpoints: Number(
                        event.target.value,
                      ),
                    })
                  }
                />
                <Input
                  label={t("execProfiles.progress.escalateAfter")}
                  type="number"
                  min={1}
                  max={100}
                  value={draft.progress.escalateAfterUnsuccessfulReplans}
                  onChange={(event) =>
                    updateProgress({
                      escalateAfterUnsuccessfulReplans: Number(
                        event.target.value,
                      ),
                    })
                  }
                />
                <Input
                  label={t("execProfiles.progress.observationTimeout")}
                  type="number"
                  min={1}
                  max={3_600}
                  value={draft.progress.observationTimeoutSecs}
                  onChange={(event) =>
                    updateProgress({
                      observationTimeoutSecs: Number(event.target.value),
                    })
                  }
                />
              </div>
              <h4>{t("execProfiles.autoStart.title")}</h4>
              <p className="muted">{t("execProfiles.autoStart.hint")}</p>
              <Checkbox
                label={t("execProfiles.autoStart.label")}
                checked={draft.autoStartEnabled}
                onChange={(event) =>
                  setDraft((current) =>
                    current
                      ? {
                          ...current,
                          autoStartEnabled: event.target.checked,
                          autoStartRiskClasses: event.target.checked
                            ? current.autoStartRiskClasses
                            : [],
                        }
                      : current,
                  )
                }
              />
              <div className="settings-checks">
                {RISK_CLASSES.map((risk) => (
                  <Checkbox
                    key={risk}
                    label={risk}
                    checked={draft.autoStartRiskClasses.includes(risk)}
                    disabled={!draft.autoStartEnabled}
                    onChange={(event) =>
                      setDraft((current) => {
                        if (!current) return current;
                        const selected = new Set(current.autoStartRiskClasses);
                        if (event.target.checked) selected.add(risk);
                        else selected.delete(risk);
                        return {
                          ...current,
                          autoStartRiskClasses: RISK_CLASSES.filter((value) =>
                            selected.has(value),
                          ),
                        };
                      })
                    }
                  />
                ))}
              </div>
            </details>

            <div className="row">
              <Button type="submit" variant="primary" disabled={busy}>
                {busy
                  ? t("execProfiles.editor.saving")
                  : draft.id
                    ? t("execProfiles.editor.saveRevision")
                    : t("execProfiles.editor.create")}
              </Button>
              <Button
                type="button"
                disabled={busy}
                onClick={() => {
                  setDraft(null);
                  setSelectedId(null);
                  setStale(false);
                  setFormError(null);
                }}
              >
                {draft.id
                  ? t("execProfiles.editor.close")
                  : t("execProfiles.editor.cancel")}
              </Button>
            </div>
          </form>
        </Panel>
      )}
    </div>
  );
}
