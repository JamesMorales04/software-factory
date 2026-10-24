export type CheckKind =
  | "format"
  | "lint"
  | "build"
  | "test"
  | "functional"
  | "negative"
  | "browser";

export interface SpecCheck {
  id: string;
  kind: CheckKind;
  description: string;
  command: string[];
  expectExit: number;
  timeoutSecs: number;
}

export interface ActionSpec {
  schema_version: number;
  project_id: string;
  repository: string;
  base_commit: string;
  objective: string;
  objective_sha256: string;
  provider: string;
  model: string;
  allowed_paths: string[];
  checks: SpecCheck[];
}

export interface Project {
  id: string;
  repository: string;
  base_commit: string;
  created_at: string;
}

export interface Action {
  id: string;
  project_id: string;
  objective: string;
  provider: string;
  model: string;
  risk: string;
  status: string;
  objective_sha256: string;
  created_at: string;
  spec: ActionSpec;
  spec_sha256: string;
}

export interface Campaign {
  id: string;
  action_id: string;
  status: string;
  created_at: string;
}

export interface Run {
  id: string;
  campaign_id: string;
  status: string;
  created_at: string;
}

export interface Attempt {
  id: string;
  run_id: string;
  status: string;
  created_at: string;
}

export interface WorkerSummary {
  worker_id: string;
  updated_at: string;
}

export interface Candidate {
  id: string;
  attempt_id: string;
  action_spec_sha256: string;
  commit_sha: string;
  tree_sha: string;
  implementer_id: string;
  created_at: string;
}

export type GateOutcome =
  | "ACCEPTED"
  | "REPAIR_REQUIRED"
  | "BLOCKED"
  | "HUMAN_REQUIRED"
  | "NOT_TESTED";

export interface Gate {
  candidate_id: string;
  outcome: GateOutcome;
  reasons: string[];
  decided_at: string;
}

export interface FactoryEvent {
  sequence: number;
  subject_id: string;
  kind: string;
  detail: string;
  recorded_at: string;
}

export interface CampaignAuthorization {
  campaign_id: string;
  authorization_id: string;
  risk_class: string;
  execution_mode: string;
  actor: string;
  rationale: string;
  authorized_paths: string[];
  max_attempts: number;
  created_at: string;
}

export interface Health {
  healthy: boolean;
  version: string;
}

export interface NewProjectRequest {
  repository: string;
  idempotency_key: string;
}

export interface NewActionRequest {
  project_id: string;
  objective: string;
  provider: string;
  model: string;
  idempotency_key: string;
  allowed_paths: string[];
  checks: SpecCheck[];
}

export interface NewCampaignRequest {
  action_id: string;
  idempotency_key: string;
}

const BASE = "/core-api";
const TOKEN_KEY = "factory-api-token";

export function getToken(): string | null {
  return sessionStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string): void {
  sessionStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  sessionStorage.removeItem(TOKEN_KEY);
}

export function newIdempotencyKey(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `key-${Date.now()}-${Math.floor(Math.random() * 1e9)}`;
}

export class CoreApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "CoreApiError";
    this.status = status;
  }
}

function messageFromBody(status: number, text: string): string {
  if (text !== "") {
    try {
      const body = JSON.parse(text) as { error?: unknown };
      if (typeof body.error === "string" && body.error !== "") {
        return body.error;
      }
    } catch {
      return text;
    }
  }
  return `Core request failed (${status}).`;
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = getToken();
  if (!token) {
    throw new CoreApiError(401, "Office is locked.");
  }
  const headers = new Headers(init.headers);
  headers.set("authorization", `Bearer ${token}`);
  if (init.body !== undefined) {
    headers.set("content-type", "application/json");
  }
  const response = await fetch(`${BASE}${path}`, { ...init, headers });
  if (!response.ok) {
    const text = await response.text();
    throw new CoreApiError(
      response.status,
      messageFromBody(response.status, text),
    );
  }
  if (response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}

export const coreApi = {
  health: () => request<Health>("/health"),
  listProjects: () => request<Project[]>("/projects"),
  registerProject: (input: NewProjectRequest) =>
    request<Project>("/projects", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  listActions: () => request<Action[]>("/actions"),
  createAction: (input: NewActionRequest) =>
    request<Action>("/actions", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  listCampaigns: () => request<Campaign[]>("/campaigns"),
  createCampaign: (input: NewCampaignRequest) =>
    request<Campaign>("/campaigns", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  authorizeFixtureCampaign: (campaignId: string) =>
    request<CampaignAuthorization>(
      `/campaigns/${encodeURIComponent(campaignId)}/authorize`,
      {
        method: "POST",
        body: JSON.stringify({
          authorization_id: `office-${campaignId}`,
          risk_class: "R0",
          execution_mode: "FIXTURE",
          actor: "office-operator",
          rationale: "deterministic fixture journey from Office",
          authorized_paths: ["src"],
          max_attempts: 1,
        }),
      },
    ),
  createCampaignRun: (campaignId: string) =>
    request<Run>(`/campaigns/${encodeURIComponent(campaignId)}/runs`, {
      method: "POST",
    }),
  createRunAttempt: (runId: string) =>
    request<Attempt>(`/runs/${encodeURIComponent(runId)}/attempts`, {
      method: "POST",
    }),
  listRuns: () => request<Run[]>("/runs"),
  listAttempts: () => request<Attempt[]>("/attempts"),
  listCandidates: () => request<Candidate[]>("/candidates"),
  listEvents: () => request<FactoryEvent[]>("/events"),
  listWorkers: () => request<WorkerSummary[]>("/workers"),
  getGate: async (candidateId: string): Promise<Gate | null> => {
    try {
      return await request<Gate>(
        `/quality/candidates/${encodeURIComponent(candidateId)}/gate`,
      );
    } catch (error) {
      if (error instanceof CoreApiError && error.status === 404) {
        return null;
      }
      throw error;
    }
  },
};
