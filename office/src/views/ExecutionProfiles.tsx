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
  type ExecutionProfile,
  type ExecutionProfileBudgets,
  type ExecutionProfileCategory,
  type ExecutionProfileRole,
  type ExecutionProfileWrite,
  type ExecutionReasoningEffort,
  type ExecutionRiskClass,
  type ExecutionRoleRoute,
  type Project,
  type ProviderModelDiscovery,
  type ProviderView,
  type ResolvedExecutionProfile,
  type SupportedExecutionRoute,
} from "../coreApi";
import { formatTimestamp } from "../format";

const ROLES: {
  id: ExecutionProfileRole;
  label: string;
  responsibility: string;
}[] = [
  {
    id: "lead",
    label: "Lead",
    responsibility: "Semantic assessment and plan proposal",
  },
  {
    id: "implementer",
    label: "Implementer",
    responsibility: "Campaign implementation session route",
  },
  {
    id: "semanticReviewer",
    label: "Semantic Reviewer",
    responsibility: "Semantic review model call, when enabled",
  },
];

const CATEGORIES: ExecutionProfileCategory[] = [
  "BUDGET",
  "BALANCED",
  "QUALITY",
  "CUSTOM",
];

const RISK_CLASSES: ExecutionRiskClass[] = ["R0", "R1", "R2", "R3"];

const REASONING: ExecutionReasoningEffort[] = ["low", "medium", "high"];

interface RoleDraft {
  provider: string;
  model: string;
  reasoning: "" | ExecutionReasoningEffort;
  maxInputTokens: number;
  maxOutputTokens: number;
  maxCostMicros: number;
}

interface ProfileDraft {
  id: string | null;
  expectedRevision: number | null;
  name: string;
  description: string;
  category: ExecutionProfileCategory;
  roles: Record<ExecutionProfileRole, RoleDraft>;
  budgets: ExecutionProfileBudgets;
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
  const role = (
    route: { provider: string; model: string },
    maxInputTokens: number,
    maxOutputTokens: number,
    maxCostMicros: number,
  ): RoleDraft => ({
    provider: route.provider,
    model: route.model,
    reasoning: "medium",
    maxInputTokens,
    maxOutputTokens,
    maxCostMicros,
  });
  return {
    id: null,
    expectedRevision: null,
    name: "Balanced",
    description: "Default supported routes",
    category: "BALANCED",
    roles: {
      lead: role(defaultRoute, 128_000, 8_192, 300_000),
      implementer: role(defaultRoute, 40_000, 12_000, 500_000),
      semanticReviewer: role(defaultRoute, 400_000, 8_192, 1_000_000),
    },
    budgets: {
      maxAttempts: 3,
      maxSessions: 2,
      maxTurns: 4,
      maxInputTokens: 40_000,
      maxOutputTokens: 12_000,
      maxCostMicros: 500_000,
      deadlineSecs: 600,
    },
    autoStartEnabled: false,
    autoStartRiskClasses: [],
  };
}

function draftFromProfile(profile: ExecutionProfile): ProfileDraft {
  const role = (route: ExecutionRoleRoute): RoleDraft => ({
    provider: route.provider,
    model: route.model,
    reasoning: route.reasoning ?? "",
    maxInputTokens: route.maxInputTokens,
    maxOutputTokens: route.maxOutputTokens,
    maxCostMicros: route.maxCostMicros,
  });
  return {
    id: profile.id,
    expectedRevision: profile.revision,
    name: profile.name,
    description: profile.description,
    category: profile.category,
    roles: {
      lead: role(profile.roles.lead),
      implementer: role(profile.roles.implementer),
      semanticReviewer: role(profile.roles.semanticReviewer),
    },
    budgets: { ...profile.budgets },
    autoStartEnabled: profile.autoStart.enabled,
    autoStartRiskClasses: [...profile.autoStart.riskClasses],
  };
}

function roleToRoute(role: RoleDraft): ExecutionRoleRoute {
  return {
    provider: role.provider,
    model: role.model,
    ...(role.reasoning === "" ? {} : { reasoning: role.reasoning }),
    maxInputTokens: role.maxInputTokens,
    maxOutputTokens: role.maxOutputTokens,
    maxCostMicros: role.maxCostMicros,
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
    budgets: { ...draft.budgets },
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

function validateModelId(model: string): string | null {
  if (model.trim() === "") return "Model ID is required.";
  if (model.length > 128) return "Model ID must be at most 128 characters.";
  if (model.trim() !== model || /\s/.test(model)) {
    return "Model ID must not contain whitespace.";
  }
  return null;
}

function routeSupportWarning(
  role: { id: ExecutionProfileRole; label: string },
  provider: ProviderView | undefined,
  model: string,
  discoveredModels: string[],
  baselineModels: string[],
): string | null {
  if (provider === undefined) {
    return `The ${role.label} provider is not a configured Core connection; this profile stays a non-executable draft.`;
  }
  if (!provider.configured) {
    return `The ${role.label} provider ${provider.provider} has no stored credential; this profile stays a non-executable draft.`;
  }
  if (!provider.enabled) {
    return `The ${role.label} provider ${provider.provider} is disabled; this profile stays a non-executable draft.`;
  }
  if (role.id !== "implementer" && provider.kind !== "opencode") {
    return `The ${role.label} route uses the ${provider.kind} App Server, which cannot execute one-shot model calls in this build; the Implementer route is unaffected.`;
  }
  const advertised = uniqueModels([
    provider.model,
    ...baselineModels,
    ...discoveredModels,
  ]);
  if (model !== "" && !advertised.includes(model)) {
    return `The ${role.label} model ${model} is not advertised by ${provider.provider}; run model discovery or select an advertised model.`;
  }
  return null;
}

function validateDraft(
  draft: ProfileDraft,
  routes: SupportedExecutionRoute[],
  providers: ProviderView[],
): string | null {
  const name = draft.name.trim();
  if (name.length === 0) return "Name is required.";
  if (name.length > 80) return "Name must be at most 80 characters.";
  if (draft.description.trim().length > 500) {
    return "Description must be at most 500 characters.";
  }
  if (draft.autoStartEnabled && draft.autoStartRiskClasses.length === 0) {
    return "Select at least one risk class when autonomous start is enabled.";
  }
  const providerIds =
    providers.length > 0
      ? providers.map((provider) => provider.provider)
      : uniqueModels(routes.map((route) => route.provider));
  for (const role of ROLES) {
    const value = draft.roles[role.id];
    if (!providerIds.includes(value.provider)) {
      return `Select a configured provider for the ${role.label} role.`;
    }
    const modelError = validateModelId(value.model);
    if (modelError) {
      return `${role.label}: ${modelError}`;
    }
    if (value.maxInputTokens < 1 || value.maxInputTokens > 1_000_000) {
      return `${role.label} max input tokens must be between 1 and 1,000,000.`;
    }
    if (value.maxOutputTokens < 1 || value.maxOutputTokens > 100_000) {
      return `${role.label} max output tokens must be between 1 and 100,000.`;
    }
    if (value.maxCostMicros < 1 || value.maxCostMicros > 100_000_000) {
      return `${role.label} max cost micros must be between 1 and 100,000,000.`;
    }
  }
  const budgets = draft.budgets;
  if (budgets.maxAttempts < 1 || budgets.maxAttempts > 10) {
    return "Max attempts must be between 1 and 10.";
  }
  if (budgets.maxSessions < 1 || budgets.maxSessions > 16) {
    return "Max sessions must be between 1 and 16.";
  }
  if (budgets.maxTurns < 1 || budgets.maxTurns > 16) {
    return "Max turns must be between 1 and 16.";
  }
  if (budgets.maxInputTokens < 1 || budgets.maxInputTokens > 1_000_000) {
    return "Campaign max input tokens must be between 1 and 1,000,000.";
  }
  if (budgets.maxOutputTokens < 1 || budgets.maxOutputTokens > 100_000) {
    return "Campaign max output tokens must be between 1 and 100,000.";
  }
  if (budgets.maxCostMicros < 1 || budgets.maxCostMicros > 100_000_000) {
    return "Campaign max cost micros must be between 1 and 100,000,000.";
  }
  if (budgets.deadlineSecs < 30 || budgets.deadlineSecs > 86_400) {
    return "Campaign deadline seconds must be between 30 and 86,400.";
  }
  return null;
}

function errorMessage(cause: unknown, fallback: string): string {
  if (cause instanceof CoreApiError) return cause.message;
  if (cause instanceof Error && cause.message !== "") return cause.message;
  return fallback;
}

export function ExecutionProfiles() {
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
      setError(errorMessage(cause, "Execution profiles could not be loaded."));
    }
  }, []);

  const discoverProvider = useCallback(async (provider: string) => {
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
          error: errorMessage(cause, "Provider model discovery failed."),
        },
      }));
    }
  }, []);

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
        errorMessage(cause, "Project profile resolution is unavailable."),
      );
    }
  }, [projectDefaultTarget]);

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
        label: `${provider.displayName} (${provider.provider})`,
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

  function updateBudget(patch: Partial<ExecutionProfileBudgets>) {
    setDraft((current) =>
      current
        ? { ...current, budgets: { ...current.budgets, ...patch } }
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
      setNotice(`Reloaded revision ${fresh.revision}.`);
      await load();
    } catch (cause) {
      setError(
        errorMessage(cause, "The current revision could not be reloaded."),
      );
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
    const validation = validateDraft(draft, routes, providers);
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
        `Saved ${saved.name} revision ${saved.revision} (${saved.executable ? "executable" : "draft, not executable"}).`,
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
        setFormError(
          errorMessage(cause, "The execution profile could not be saved."),
        );
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
          ? `Global default set to ${profile.name}.`
          : `Project default set to ${profile.name}.`,
      );
      await load();
      await refreshResolution();
    } catch (cause) {
      setError(errorMessage(cause, "The profile default could not be set."));
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
          ? "Global default cleared."
          : "Project default cleared.",
      );
      await load();
      await refreshResolution();
    } catch (cause) {
      setError(
        errorMessage(cause, "The profile default could not be cleared."),
      );
    } finally {
      setBusy(false);
    }
  }

  if (!loaded) {
    if (error) {
      return (
        <ErrorState
          title="Execution profiles unavailable"
          message={error}
          action={<Button onClick={() => void load()}>Retry</Button>}
        />
      );
    }
    return <LoadingState label="Loading execution profiles…" />;
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
            <h2>Execution profiles ({profiles.length})</h2>
            <p className="muted">
              Versioned operator configuration for the model-backed roles. A
              profile is not an authorization or a capability grant.
            </p>
          </div>
          <div className="row">
            <Button
              variant="primary"
              size="sm"
              disabled={busy || routes.length === 0}
              onClick={startNewProfile}
            >
              New profile
            </Button>
            <Button size="sm" disabled={busy} onClick={() => void load()}>
              Refresh
            </Button>
          </div>
        </div>

        <p className="muted">
          Fetched {lastLoadedAt ? formatTimestamp(lastLoadedAt) : "—"} ·{" "}
          {routes.length} baseline tested route(s) · {providers.length} provider
          connection(s)
        </p>

        {error && (
          <div className="ui-callout ui-callout--danger" role="alert">
            <p>{error} The displayed profiles may be stale.</p>
            <Button size="sm" onClick={() => void load()}>
              Retry
            </Button>
          </div>
        )}
        {notice && <p className="notice">{notice}</p>}

        {profiles.length === 0 ? (
          <EmptyState
            title="No execution profiles"
            description="Create a profile from the Core-supported routes. A draft with an unconfigured provider is saved as non-executable and cannot become a default."
            action={
              <Button
                variant="primary"
                disabled={busy || routes.length === 0}
                onClick={startNewProfile}
              >
                Create execution profile
              </Button>
            }
          />
        ) : (
          <div className="execution-profiles-table">
            <table>
              <thead>
                <tr>
                  <th scope="col">Profile</th>
                  <th scope="col">Category</th>
                  <th scope="col">Revision</th>
                  <th scope="col">Content hash</th>
                  <th scope="col">Executable</th>
                  <th scope="col">Defaults</th>
                  <th scope="col">Action</th>
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
                    </th>
                    <td>
                      <Badge tone="neutral">{profile.category}</Badge>
                    </td>
                    <td>
                      {profile.revision}
                      <span className="execution-profile-role-note">
                        <code>{profile.revisionId}</code>
                      </span>
                    </td>
                    <td>
                      <code className="execution-profile-hash">
                        {profile.contentHash}
                      </code>
                    </td>
                    <td>
                      <Badge
                        tone={profile.executable ? "success" : "attention"}
                      >
                        {profile.executable ? "EXECUTABLE" : "DRAFT"}
                      </Badge>
                      {profile.warnings.length > 0 && (
                        <span className="execution-profile-role-note">
                          {profile.warnings.length} warning(s)
                        </span>
                      )}
                    </td>
                    <td>
                      {profile.globalDefault && (
                        <Badge tone="info">GLOBAL DEFAULT</Badge>
                      )}
                      {profile.projectDefaults.map((projectId) => (
                        <Badge key={projectId} tone="neutral">
                          PROJECT {projectId.slice(0, 8)}
                        </Badge>
                      ))}
                      {!profile.globalDefault &&
                        profile.projectDefaults.length === 0 && (
                          <span className="muted">none</span>
                        )}
                    </td>
                    <td>
                      <Button size="sm" onClick={() => selectProfile(profile)}>
                        Edit
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
            aria-label="Execution profile defaults"
          >
            <h3>Defaults and effective profile</h3>
            <div className="execution-profile-meta">
              <Badge
                tone={selectedProfile.executable ? "success" : "attention"}
              >
                {selectedProfile.executable ? "EXECUTABLE" : "DRAFT"}
              </Badge>
              <span>
                Revision <strong>{selectedProfile.revision}</strong>
              </span>
              <span>
                Revision ID <code>{selectedProfile.revisionId}</code>
              </span>
              <span>
                Content hash{" "}
                <code className="execution-profile-hash">
                  {selectedProfile.contentHash}
                </code>
              </span>
            </div>
            {selectedProfile.warnings.length > 0 && (
              <div className="ui-callout ui-callout--attention" role="status">
                <p>
                  This profile is a non-executable draft. Core will reject it as
                  a default or execution route until every warning is resolved.
                </p>
                <ul>
                  {selectedProfile.warnings.map((warning) => (
                    <li key={warning}>{warning}</li>
                  ))}
                </ul>
              </div>
            )}
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
                    Clear global default
                  </Button>
                ) : (
                  <Button
                    disabled={busy || !selectedProfile.executable}
                    onClick={() => {
                      if (selectedProfile)
                        void setDefault(selectedProfile, "global");
                    }}
                  >
                    Set global default
                  </Button>
                )}
              </div>
              <div className="execution-profile-project-default">
                <Select
                  label="Default project"
                  value={projectDefaultTarget}
                  onChange={(event) =>
                    setProjectDefaultTarget(event.target.value)
                  }
                  disabled={projects.length === 0}
                  help="A project default overrides the global default for that project only."
                >
                  {projects.length === 0 && (
                    <option value="">No registered projects</option>
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
                      Clear project default
                    </Button>
                  ) : (
                    <Button
                      disabled={busy || !selectedProfile.executable}
                      onClick={() => {
                        if (selectedProfile)
                          void setDefault(selectedProfile, "project");
                      }}
                    >
                      Set project default
                    </Button>
                  ))}
              </div>
            </div>
            <div className="execution-profile-resolution" role="status">
              {projectDefaultTarget === "" ? (
                <p className="muted">
                  Register a project to inspect project-default resolution.
                </p>
              ) : resolutionError ? (
                <p className="error">
                  Project resolution failed: {resolutionError}
                </p>
              ) : resolution ? (
                <p>
                  Effective for{" "}
                  {defaultProject?.repository ?? projectDefaultTarget}: profile{" "}
                  <code>{resolution.profileId}</code>, revision{" "}
                  {resolution.revision}, source{" "}
                  <Badge tone="info">{resolution.source}</Badge>
                </p>
              ) : (
                <p className="muted">
                  No execution profile default exists for this project.
                </p>
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
                  {draft.id ? `Edit ${draft.name}` : "New execution profile"}
                </h2>
                <p className="muted">
                  Every save creates an immutable revision. The expected
                  revision prevents overwriting a newer operator edit.
                </p>
              </div>
              {draft.id && (
                <span className="muted">
                  Editing revision {draft.expectedRevision}
                </span>
              )}
            </div>

            {stale && (
              <div className="ui-callout ui-callout--danger" role="alert">
                <p>
                  Core rejected this save because the profile changed since it
                  was loaded. No revision was written.
                </p>
                <Button
                  size="sm"
                  disabled={busy}
                  onClick={() => void reloadCurrentRevision()}
                >
                  Reload current revision
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
                <p>
                  This draft is not executable. Core refuses to bind or use a
                  profile until every role route is substantiated.
                </p>
                <ul>
                  {draftWarnings().map((warning) => (
                    <li key={warning}>{warning}</li>
                  ))}
                </ul>
              </div>
            )}

            <div className="grid-2">
              <Input
                label="Name"
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
                label="Category"
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
                    {category}
                  </option>
                ))}
              </Select>
            </div>
            <Textarea
              label="Description"
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

            <section aria-label="Role matrix">
              <div className="section-heading">
                <div>
                  <h3>Role matrix</h3>
                  <span className="muted">
                    Each role resolves its own provider connection and model.
                    The deterministic Verifier is not a profile role.
                  </span>
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
                  Refresh model catalog
                </Button>
              </div>
              <div className="execution-profile-matrix">
                <table>
                  <thead>
                    <tr>
                      <th scope="col">Role</th>
                      <th scope="col">Provider</th>
                      <th scope="col">Model</th>
                      <th scope="col">Reasoning</th>
                      <th scope="col">Max input</th>
                      <th scope="col">Max output</th>
                      <th scope="col">Max cost µ</th>
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
                            <strong>{role.label}</strong>
                            <span className="execution-profile-role-note">
                              {role.responsibility}
                            </span>
                          </th>
                          <td>
                            <Select
                              label={`${role.label} provider`}
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
                                  {value.provider} (not configured)
                                </option>
                              )}
                            </Select>
                            <span className="execution-profile-role-note">
                              {entry === "loading"
                                ? "Discovering models…"
                                : error
                                  ? `Model catalog unavailable: ${error}`
                                  : isDiscovery(entry)
                                    ? entry.status === "AVAILABLE"
                                      ? `${entry.models.length} model(s) discovered`
                                      : `Discovery unavailable: ${entry.detail}`
                                    : configured
                                      ? "Model catalog not loaded"
                                      : "Provider is not configured"}
                            </span>
                          </td>
                          <td>
                            {!manual && (
                              <>
                                <Input
                                  label={`Search ${role.label} models`}
                                  value={modelFilters[role.id]}
                                  placeholder="Filter discovered models"
                                  onChange={(event) =>
                                    setModelFilters((current) => ({
                                      ...current,
                                      [role.id]: event.target.value,
                                    }))
                                  }
                                />
                                <Select
                                  label={`${role.label} model`}
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
                                        ? "No advertised model"
                                        : `${value.model} (unverified)`}
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
                                label={`${role.label} manual model ID (unverified)`}
                                value={value.model}
                                placeholder="provider-model-id"
                                help="Core keeps a profile non-executable until the provider advertises this exact model."
                                onChange={(event) =>
                                  updateRole(role.id, {
                                    model: event.target.value,
                                  })
                                }
                              />
                            )}
                            <Checkbox
                              label={`Advanced: manual ${role.label} model ID (unverified)`}
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
                              label={`${role.label} reasoning`}
                              value={value.reasoning}
                              onChange={(event) =>
                                updateRole(role.id, {
                                  reasoning: event.target
                                    .value as RoleDraft["reasoning"],
                                })
                              }
                            >
                              <option value="">Provider default</option>
                              {REASONING.map((reasoning) => (
                                <option key={reasoning} value={reasoning}>
                                  {reasoning}
                                </option>
                              ))}
                            </Select>
                          </td>
                          <td>
                            <Input
                              label={`${role.label} max input tokens`}
                              type="number"
                              min={1}
                              max={1_000_000}
                              value={value.maxInputTokens}
                              onChange={(event) =>
                                updateRole(role.id, {
                                  maxInputTokens: Number(event.target.value),
                                })
                              }
                            />
                          </td>
                          <td>
                            <Input
                              label={`${role.label} max output tokens`}
                              type="number"
                              min={1}
                              max={100_000}
                              value={value.maxOutputTokens}
                              onChange={(event) =>
                                updateRole(role.id, {
                                  maxOutputTokens: Number(event.target.value),
                                })
                              }
                            />
                          </td>
                          <td>
                            <Input
                              label={`${role.label} max cost micros`}
                              type="number"
                              min={1}
                              max={100_000_000}
                              value={value.maxCostMicros}
                              onChange={(event) =>
                                updateRole(role.id, {
                                  maxCostMicros: Number(event.target.value),
                                })
                              }
                            />
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </section>

            <section aria-label="Campaign budgets">
              <div className="section-heading">
                <h3>Campaign budgets</h3>
                <span className="muted">
                  Frozen into a Campaign authorization; a model cannot widen
                  them.
                </span>
              </div>
              <div className="grid-2">
                <Input
                  label="Max attempts"
                  type="number"
                  min={1}
                  max={10}
                  value={draft.budgets.maxAttempts}
                  onChange={(event) =>
                    updateBudget({ maxAttempts: Number(event.target.value) })
                  }
                />
                <Input
                  label="Max sessions"
                  type="number"
                  min={1}
                  max={16}
                  value={draft.budgets.maxSessions}
                  onChange={(event) =>
                    updateBudget({ maxSessions: Number(event.target.value) })
                  }
                />
                <Input
                  label="Max turns"
                  type="number"
                  min={1}
                  max={16}
                  value={draft.budgets.maxTurns}
                  onChange={(event) =>
                    updateBudget({ maxTurns: Number(event.target.value) })
                  }
                />
                <Input
                  label="Campaign max input tokens"
                  type="number"
                  min={1}
                  max={1_000_000}
                  value={draft.budgets.maxInputTokens}
                  onChange={(event) =>
                    updateBudget({ maxInputTokens: Number(event.target.value) })
                  }
                />
                <Input
                  label="Campaign max output tokens"
                  type="number"
                  min={1}
                  max={100_000}
                  value={draft.budgets.maxOutputTokens}
                  onChange={(event) =>
                    updateBudget({
                      maxOutputTokens: Number(event.target.value),
                    })
                  }
                />
                <Input
                  label="Campaign max cost micros"
                  type="number"
                  min={1}
                  max={100_000_000}
                  value={draft.budgets.maxCostMicros}
                  onChange={(event) =>
                    updateBudget({ maxCostMicros: Number(event.target.value) })
                  }
                />
                <Input
                  label="Campaign deadline seconds"
                  type="number"
                  min={30}
                  max={86_400}
                  value={draft.budgets.deadlineSecs}
                  onChange={(event) =>
                    updateBudget({ deadlineSecs: Number(event.target.value) })
                  }
                />
              </div>
            </section>

            <section aria-label="Autonomous start policy">
              <div className="section-heading">
                <h3>Autonomous start policy</h3>
                <span className="muted">
                  This is a policy declaration, not authority. Core still
                  requires the frozen plan, delegation and risk approval.
                </span>
              </div>
              <Checkbox
                label="Declare autonomous start eligible for selected risk classes"
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
            </section>

            <div className="row">
              <Button type="submit" variant="primary" disabled={busy}>
                {busy
                  ? "Saving…"
                  : draft.id
                    ? "Save revision"
                    : "Create profile"}
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
                {draft.id ? "Close editor" : "Cancel"}
              </Button>
            </div>
          </form>
        </Panel>
      )}
    </div>
  );
}
