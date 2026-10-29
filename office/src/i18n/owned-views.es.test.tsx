import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  type Action,
  type CampaignDetail as CampaignDetailData,
  coreApi,
  type DelegationProjection,
  type OfficeOverview,
  type Project,
  type ProjectFileProjection,
  type ProjectRefsProjection,
  type ProjectTreeProjection,
  type ProviderView,
  type StandingDelegation,
} from "../coreApi";
import { I18nProvider } from "../i18n";
import { ActionDetail } from "../views/ActionDetail";
import { Actions } from "../views/Actions";
import { CampaignDetail } from "../views/CampaignDetail";
import { Campaigns } from "../views/Campaigns";
import { Delegations } from "../views/Delegations";
import { ExecutionProfiles } from "../views/ExecutionProfiles";
import { Lead } from "../views/Lead";
import { Overview } from "../views/Overview";
import { ProjectFiles } from "../views/ProjectFiles";
import { Settings } from "../views/Settings";

vi.mock("../coreApi", () => {
  class CoreApiError extends Error {
    readonly status = 500;
    readonly code = null;
    readonly reason = null;
    readonly body = null;
  }
  return {
    CoreApiError,
    newIdempotencyKey: () => "idem-test",
    coreApi: {
      listActions: vi.fn().mockResolvedValue([]),
      listProjects: vi.fn().mockResolvedValue([]),
      listProviders: vi
        .fn()
        .mockResolvedValue({ providers: [], supportedRoutes: [] }),
      listCampaigns: vi.fn().mockResolvedValue([]),
      listLeadRequests: vi.fn().mockResolvedValue([]),
      listDelegations: vi.fn().mockResolvedValue([]),
      listCandidates: vi.fn().mockResolvedValue([]),
      listAttempts: vi.fn().mockResolvedValue([]),
      listRuns: vi.fn().mockResolvedValue([]),
      getGate: vi.fn().mockResolvedValue(null),
      getProjectConfig: vi.fn().mockResolvedValue({
        configHash: "config-hash",
      }),
      getOverview: vi.fn().mockResolvedValue(null),
      getDelegation: vi.fn().mockResolvedValue({
        usage: { requests: 2, sessions: 1, attempts: 1 },
      }),
      getCampaignDetail: vi.fn().mockResolvedValue(null),
      getCandidateReceipts: vi.fn().mockResolvedValue([]),
      listProviderSessions: vi.fn().mockResolvedValue([]),
      getLeadRequest: vi.fn().mockResolvedValue(null),
      getCandidateDiff: vi.fn().mockResolvedValue(null),
      getProjectGitHub: vi
        .fn()
        .mockResolvedValue({ binding: null, sync_state: null }),
      listProjectRefs: vi.fn().mockResolvedValue(null),
      listProjectFiles: vi.fn().mockResolvedValue(null),
      getProjectFile: vi.fn().mockResolvedValue(null),
      getProjectSummary: vi.fn().mockResolvedValue(null),
      getProjectDiscovery: vi.fn().mockResolvedValue({
        discovery: null,
        current_base_commit: "",
        stale: false,
      }),
      getRuntimeSettings: vi.fn().mockResolvedValue({
        semanticReviewEnabled: false,
        semanticReviewMaxOutputTokens: 8192,
        updatedAt: "2026-10-27T10:00:00Z",
      }),
      listExecutionProfiles: vi
        .fn()
        .mockResolvedValue({ profiles: [], supportedRoutes: [] }),
      getProjectExecutionProfile: vi.fn().mockResolvedValue(null),
      getProjectExecutionProfileDefault: vi.fn(),
    },
  };
});

function renderRoutes(
  entry: string,
  path: string,
  element: ReactElement,
  locale: "en" | "es",
) {
  return render(
    <I18nProvider initialLocale={locale}>
      <MemoryRouter initialEntries={[entry]}>
        <Routes>
          <Route path={path} element={element} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>,
  );
}

describe("localized operational views", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("renders Actions in English and Spanish without translating machine IDs", async () => {
    const english = renderRoutes("/actions", "/actions", <Actions />, "en");
    expect(
      await screen.findByRole("heading", { name: "Actions" }),
    ).toBeDefined();
    expect(
      screen.getByText(/Register a Project before creating an Action/),
    ).toBeDefined();
    english.unmount();

    renderRoutes("/actions", "/actions", <Actions />, "es");
    expect(
      await screen.findByRole("heading", { name: "Acciones" }),
    ).toBeDefined();
    expect(screen.getByText("Todas las Acciones (0)")).toBeDefined();
    expect(
      screen.getByText(/Registra un Proyecto antes de crear una Acción/),
    ).toBeDefined();
  });

  it("renders Action detail not-found states in both locales", async () => {
    const english = renderRoutes(
      "/actions/action-1",
      "/actions/:actionId",
      <ActionDetail />,
      "en",
    );
    expect(await screen.findByText("Action not found")).toBeDefined();
    expect(
      screen.getByText("No Action action-1 is recorded in Core."),
    ).toBeDefined();
    english.unmount();

    renderRoutes(
      "/actions/action-1",
      "/actions/:actionId",
      <ActionDetail />,
      "es",
    );
    expect(await screen.findByText("Acción no encontrada")).toBeDefined();
    expect(
      screen.getByText("No hay ninguna Acción action-1 registrada en Core."),
    ).toBeDefined();
    expect(
      screen.getByRole("link", { name: "Volver a Acciones" }),
    ).toBeDefined();
  });

  it("renders Campaigns and Lead in both locales", async () => {
    const englishCampaigns = renderRoutes(
      "/campaigns",
      "/campaigns",
      <Campaigns />,
      "en",
    );
    expect(
      await screen.findByRole("heading", { name: "Campaigns" }),
    ).toBeDefined();
    expect(
      screen.getByText(/Create an Action before starting a Campaign/),
    ).toBeDefined();
    englishCampaigns.unmount();

    const spanishCampaigns = renderRoutes(
      "/campaigns",
      "/campaigns",
      <Campaigns />,
      "es",
    );
    expect(
      await screen.findByRole("heading", { name: "Campañas" }),
    ).toBeDefined();
    expect(
      screen.getByText(/Crea una Acción antes de iniciar una Campaña/),
    ).toBeDefined();
    spanishCampaigns.unmount();

    renderRoutes("/lead", "/lead", <Lead />, "es");
    expect(await screen.findByRole("heading", { name: "Lead" })).toBeDefined();
    expect(screen.getByText("Enviar una solicitud de Lead")).toBeDefined();
    expect(screen.getByText("No hay solicitudes de Lead.")).toBeDefined();
  });
});

const project: Project = {
  id: "project-1",
  repository: "/tmp/project",
  base_commit: "a".repeat(40),
  created_at: "2026-10-27T09:00:00Z",
  identity: "b".repeat(64),
};

const overview: OfficeOverview = {
  as_of: "2026-10-27T10:00:00Z",
  source: "Core durable snapshots",
  counts: {
    projects: 1,
    actions: 1,
    campaigns: 1,
    runs: 0,
    attempts: 0,
    candidates: 1,
    workers: 1,
    lead_requests: 0,
    authorized_campaigns: 1,
    campaigns_awaiting_authorization: 0,
    active_agents: 0,
    unresolved_sessions: 0,
    unresolved_model_calls: 0,
    pending_decisions: 1,
    blocked_signals: 0,
  },
  actions_by_status: { AUTHORIZED: 1 },
  campaigns_by_status: { AUTHORIZED: 1 },
  runs_by_status: {},
  gate_outcomes: { ACCEPTED: 1 },
  budget: {
    measured_session_cost_micros: 1914,
    measured_session_tokens: 100,
    measured_model_cost_micros: 0,
    measured_model_tokens: 0,
    unmeasured_sessions: 0,
    unmeasured_model_calls: 0,
    unmeasured_bound_cost_micros: 500000,
    model_calls_sampled: 0,
    note: "Measured records only.",
  },
  runtime: {
    core_healthy: true,
    workers: 1,
    active_agents: 0,
    unresolved_sessions: 0,
    unresolved_model_calls: 0,
    observability: {
      enabled: false,
      cursor: 0,
      exported: 0,
      pending: 0,
      dropped: 0,
      failed_attempts: 0,
      last_error: null,
    },
  },
  recent_accepted: [
    {
      candidate_id: "candidate-1",
      campaign_id: "campaign-1",
      commit_sha: "c".repeat(40),
      decided_at: "2026-10-27T09:30:00Z",
      outcome: "ACCEPTED",
    },
  ],
  recent_decisions: [],
  recent_events: [
    {
      sequence: 7,
      subject_id: "campaign-1",
      kind: "campaign.authorized",
      detail: "Core recorded the authorization event verbatim.",
      recorded_at: "2026-10-27T09:10:00Z",
    },
  ],
};

const delegation: StandingDelegation = {
  id: "delegation-1",
  principalId: "owner",
  auditId: "office-delegation",
  projectId: "project-1",
  revision: 1,
  contentHash: "",
  objectiveClasses: ["feature"],
  allowedPaths: ["src"],
  capabilities: ["repository-edit"],
  riskClasses: ["R0"],
  autonomousStart: false,
  autonomousRiskClasses: [],
  route: {
    provider: "opencode-go",
    model: "deepseek-v4.1-flash",
    mode: "build",
  },
  maxAttempts: 2,
  maxTimeoutSecs: 300,
  maxRepairCycles: 0,
  maxWallClockSecs: 1800,
  maxParallel: 1,
  maxSessions: 16,
  maxRequests: 4,
  maxCumulativeSessions: 32,
  maxCumulativeAttempts: 32,
  expiresAt: "2026-10-28T10:00:00Z",
};

const action: Action = {
  id: "action-1",
  project_id: "project-1",
  objective: "Modernize the Campaign workspace.",
  provider: "opencode",
  model: "deepseek-v4.1-flash",
  risk: "R0",
  status: "AUTHORIZED",
  objective_sha256: "objective",
  created_at: "2026-10-27T10:00:00Z",
  spec_sha256: "spec",
  spec: {
    schema_version: 1,
    project_id: "project-1",
    repository: "/tmp/project",
    base_commit: "a".repeat(40),
    objective: "Modernize the Campaign workspace.",
    objective_sha256: "objective",
    provider: "opencode",
    model: "deepseek-v4.1-flash",
    allowed_paths: ["src"],
    checks: [],
  },
};

const campaignDetail: CampaignDetailData = {
  campaign: {
    id: "campaign-1",
    action_id: "action-1",
    status: "AUTHORIZED",
    created_at: "2026-10-27T10:00:00Z",
  },
  authorization: null,
  tasks: [
    {
      task_id: "task-1",
      campaign_id: "campaign-1",
      request_id: "lead-1",
      unit_id: "unit-a",
      sequence: 0,
      status: "PENDING",
      base_commit: "a".repeat(40),
      depends_on: [],
      run_id: null,
      attempt_id: null,
      candidate_id: null,
      candidate_commit: null,
      candidate_tree: null,
      updated_at: "2026-10-27T10:00:00Z",
    },
  ],
  runs: [],
  attempts: [],
  candidates: [],
  events: [],
};

const refs: ProjectRefsProjection = {
  projectId: "project-1",
  baseCommit: project.base_commit,
  head: "main",
  refs: [{ name: "main", kind: "BRANCH", commit: project.base_commit }],
  truncated: false,
  provenance: "Core read-only Git projection",
};

const rootTree: ProjectTreeProjection = {
  projectId: "project-1",
  revision: { commit: project.base_commit, tree: "d".repeat(40) },
  path: "",
  entries: [
    {
      name: "README.md",
      path: "README.md",
      kind: "FILE",
      mode: "100644",
      objectId: "f".repeat(40),
      size: 34,
    },
  ],
  truncated: false,
  skippedEntries: 0,
  provenance: "Core read-only Git projection",
};

const readmeFile: ProjectFileProjection = {
  projectId: "project-1",
  revision: { commit: project.base_commit, tree: "d".repeat(40) },
  path: "README.md",
  name: "README.md",
  kind: "FILE",
  mode: "100644",
  objectId: "f".repeat(40),
  size: 34,
  content: "# Documentación\n",
  omitted: null,
  lineCount: 1,
  provenance: "Core read-only Git projection",
};

const provider: ProviderView = {
  provider: "codex",
  kind: "codex",
  displayName: "Codex",
  defaultModel: "gpt-6-luna",
  defaultBaseUrl: "https://api.openai.com/v1",
  authModes: ["API_KEY"],
  chatgptOauth: "BLOCKED",
  status: "DETERMINISTIC_ONLY",
  note: "Codex App Server route",
  configured: true,
  enabled: true,
  isDefault: false,
  model: "gpt-6-luna",
  baseUrl: "https://api.openai.com/v1",
  credentialFingerprint: "fingerprint",
  revision: 3,
  updatedAt: "2026-10-27T10:00:00Z",
  maxTurns: 4,
  maxInputTokens: 60000,
  maxOutputTokens: 12000,
  maxCostMicros: 500000,
  inputMicrosPerMillion: 1000,
  outputMicrosPerMillion: 2000,
  modelDiscovery: null,
};

describe("newly translated operational surfaces", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.lang = "es";
    vi.mocked(coreApi.getOverview).mockResolvedValue(overview);
    vi.mocked(coreApi.listProjects).mockResolvedValue([project]);
    vi.mocked(coreApi.listDelegations).mockResolvedValue([
      {
        schemaVersion: 1,
        historical: true,
        delegation,
        usage: {
          delegation_id: delegation.id,
          requests: 2,
          sessions: 1,
          attempts: 1,
        },
      } satisfies DelegationProjection,
    ]);
    vi.mocked(coreApi.getDelegation).mockResolvedValue({
      delegation,
      usage: {
        delegation_id: delegation.id,
        requests: 2,
        sessions: 1,
        attempts: 1,
      },
      schemaVersion: 1,
      historical: true,
    });
    vi.mocked(coreApi.getCampaignDetail).mockResolvedValue(campaignDetail);
    vi.mocked(coreApi.listActions).mockResolvedValue([action]);
    vi.mocked(coreApi.listLeadRequests).mockResolvedValue([]);
    vi.mocked(coreApi.listProviderSessions).mockResolvedValue([]);
    vi.mocked(coreApi.listProjectRefs).mockResolvedValue(refs);
    vi.mocked(coreApi.listProjectFiles).mockResolvedValue(rootTree);
    vi.mocked(coreApi.getProjectFile).mockResolvedValue(readmeFile);
    vi.mocked(coreApi.listProviders).mockResolvedValue({
      providers: [provider],
    });
    vi.mocked(coreApi.getRuntimeSettings).mockResolvedValue({
      semanticReviewEnabled: false,
      semanticReviewMaxOutputTokens: 8192,
      updatedAt: "2026-10-27T10:00:00Z",
    });
    vi.mocked(coreApi.listExecutionProfiles).mockResolvedValue({
      profiles: [],
      supportedRoutes: [],
    });
  });

  it("renders the Spanish Overview while preserving raw Core records", async () => {
    renderRoutes("/overview", "/overview", <Overview />, "es");
    expect(
      await screen.findByRole("heading", { name: "Resumen" }),
    ).toBeDefined();
    expect(screen.getByText("Campañas autorizadas")).toBeDefined();
    expect(screen.getByText("Gasto medido")).toBeDefined();
    expect(
      screen.getByText("Resultados de la Puerta de calidad"),
    ).toBeDefined();
    expect(screen.getByText("Bandeja de decisiones")).toBeDefined();
    expect(screen.queryByText("campaña.authorized")).toBeNull();
    expect(screen.getByText("campaign.authorized")).toBeDefined();
    expect(
      screen.getByText("Core recorded the authorization event verbatim."),
    ).toBeDefined();
    expect(screen.getByText("ACCEPTED")).toBeDefined();
  });

  it("renders the Spanish Delegations form and table without translating IDs", async () => {
    renderRoutes("/delegations", "/delegations", <Delegations />, "es");
    expect(
      await screen.findByRole("heading", { name: "Delegaciones" }),
    ).toBeDefined();
    expect(screen.getByText("Nueva delegación")).toBeDefined();
    expect(screen.getByText("Delegaciones (1)")).toBeDefined();
    expect(
      screen.getByText("Rutas permitidas (separadas por comas)"),
    ).toBeDefined();
    expect(screen.getByText("HISTORICAL")).toBeDefined();
    expect(screen.queryByRole("button", { name: "Revocar" })).toBeNull();
    expect(screen.getByText("delegation-1")).toBeDefined();
    expect(screen.getByText(/deepseek-v4\.1-flash/)).toBeDefined();
    expect(screen.getByText("2 sol · 1 ses · 1 int")).toBeDefined();
  });

  it("renders the Spanish Campaign detail tabs, WorkGraph and cancellation affordance", async () => {
    renderRoutes(
      "/campaigns/campaign-1",
      "/campaigns/:campaignId",
      <CampaignDetail />,
      "es",
    );
    expect(
      await screen.findByRole("heading", { name: "Campaña campaign" }),
    ).toBeDefined();
    expect(
      screen.getByRole("heading", { name: "Panorama operativo" }),
    ).toBeDefined();
    expect(
      screen.getByText("No hay señales de atención del operador registradas"),
    ).toBeDefined();
    expect(screen.getByRole("tab", { name: /WorkGraph/ })).toBeDefined();
    expect(
      screen.getByRole("tab", { name: /Presupuesto y uso/ }),
    ).toBeDefined();
    expect(
      screen.getByRole("button", { name: "Cancelar Campaña" }),
    ).toBeDefined();
    expect(screen.getAllByText("AUTHORIZED").length).toBeGreaterThan(0);

    await userEvent.click(screen.getByRole("tab", { name: /WorkGraph/ }));
    expect(
      await screen.findByRole("heading", {
        name: "WorkGraph (1 unidad)",
      }),
    ).toBeDefined();

    await userEvent.click(
      screen.getByRole("tab", { name: /Presupuesto y uso/ }),
    );
    expect(
      screen.getByText(
        "No hay autorización de Campaña registrada, por lo que no hay presupuesto de ejecución vigente.",
      ),
    ).toBeDefined();

    await userEvent.click(screen.getByRole("tab", { name: /Eventos/ }));
    expect(
      screen.getByRole("heading", { name: "Cancelación y recuperación" }),
    ).toBeDefined();
  });

  it("renders the Spanish Project files browser and keeps paths and Git data raw", async () => {
    renderRoutes(
      "/projects/project-1/files",
      "/projects/:projectId/files",
      <ProjectFiles />,
      "es",
    );
    expect(
      await screen.findByRole("heading", { name: "Archivos del Proyecto" }),
    ).toBeDefined();
    expect(screen.getByText("Base congelada")).toBeDefined();
    expect(screen.getByText("Árbol del repositorio")).toBeDefined();
    expect(await screen.findByText(/Mostrando 1 de 1 entradas/)).toBeDefined();
    expect(
      screen.getByRole("button", { name: "Abrir archivo README.md" }),
    ).toBeDefined();
    expect(screen.getByText("Selecciona un archivo")).toBeDefined();

    await userEvent.click(
      screen.getByRole("button", { name: "Abrir archivo README.md" }),
    );
    expect(
      await screen.findByRole("heading", { name: "README.md" }),
    ).toBeDefined();
    expect(screen.getByText("Ruta")).toBeDefined();
    expect(screen.getByText("Procedencia")).toBeDefined();
    expect(screen.getByRole("button", { name: "Vista previa" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Código" })).toBeDefined();
  });

  it("renders the Spanish Settings provider chrome without translating provider IDs", async () => {
    renderRoutes("/settings", "/settings", <Settings />, "es");
    expect(
      await screen.findByRole("heading", { name: "Configuración" }),
    ).toBeDefined();
    expect(
      screen.getByRole("tab", { name: /Proveedores y modelos/ }),
    ).toBeDefined();
    expect(screen.getAllByText("Catálogo de modelos").length).toBeGreaterThan(
      0,
    );
    expect(
      screen.getByRole("button", { name: "Descubrir modelos" }),
    ).toBeDefined();
    expect(
      screen.getByRole("button", { name: "Revocar credencial" }),
    ).toBeDefined();
    expect(screen.getByText("credencial configurada")).toBeDefined();
    expect(screen.getByText("codex")).toBeDefined();
    expect(screen.getByLabelText("Modelo")).toBeDefined();
    expect(screen.getByText("Modelo actual")).toBeDefined();
    expect(screen.getByText("Ruta manual avanzada")).toBeDefined();
  });

  it("renders the Spanish Execution Profiles editor chrome", async () => {
    renderRoutes(
      "/settings?tab=profiles",
      "/settings",
      <ExecutionProfiles />,
      "es",
    );
    expect(
      await screen.findByRole("heading", { name: "Perfiles de ejecución (0)" }),
    ).toBeDefined();
    expect(screen.getByRole("button", { name: "Nuevo perfil" })).toBeDefined();
    expect(screen.getByText("Sin perfiles de ejecución")).toBeDefined();
    expect(
      screen.getByRole("button", { name: "Crear perfil de ejecución" }),
    ).toBeDefined();
  });
});
