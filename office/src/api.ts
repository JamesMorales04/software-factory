// Factory Office API client. All state is server-authoritative: the UI
// renders Core snapshots and the durable event ledger, never invented data.

export type ActionStatus =
  | "pending"
  | "in_progress"
  | "completed"
  | "cancelled";

export type Capability =
  | "repository-edit"
  | "node-toolchain"
  | "generic-toolchain"
  | "browser";

export interface Project {
  id: string;
  name: string;
  registrationStatus: "legacy" | "registered";
  status: "active" | "paused";
  repositoryPath?: string;
  repositoryIdentity?: string;
  baseRefs: string[];
  qualityProfile: string;
  workspaceScope: string;
  capabilities: Capability[];
  browserEndpoints: string[];
  executionMode: string;
  securityLabel: string;
  configRevision: number;
  configHash: string;
  registeredAt?: string;
  createdAt: string;
  updatedAt: string;
}

export interface Action {
  id: string;
  projectId: string;
  title: string;
  description: string;
  status: ActionStatus;
  idempotencyKey?: string;
  createdAt: string;
  updatedAt: string;
}

export interface FactoryEvent {
  eventId: string;
  eventType: string;
  schemaVersion: string;
  aggregateType: string;
  aggregateId: string;
  sequence: number;
  globalSeq: number;
  occurredAt: string;
  recordedAt: string;
  correlationId: string;
  causationId: string;
  actor: string;
  payload: Record<string, unknown>;
}

export interface Health {
  status: string;
  version: string;
  time: string;
}

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

export interface Run {
  id: string;
  projectId: string;
  actionId?: string;
  workloadKind: string;
  workloadInput: Record<string, unknown>;
  status: string;
  attemptCount: number;
  maxAttempts: number;
  timeoutSecs: number;
  leaseOwner?: string;
  leaseGeneration: number;
  leaseExpiresAt?: string;
  result?: Record<string, unknown>;
  error?: string;
  // S3-A: requested execution mode and pinned Paseo routing. Absent for S2
  // synthetic runs (restricted). Rendered read-only alongside the S3-C
  // session/evidence projection.
  executionMode: string;
  provider?: string;
  model?: string;
  paseoMode?: string;
  specId?: string;
  createdAt: string;
  updatedAt: string;
}

export interface PaseoRecovery {
  attemptId: string;
  runId: string;
  leaseGeneration: number;
  status: string;
  claimOwner?: string;
  claimExpiresAt?: string;
  attempts: number;
  nextAttemptAt?: string;
  lastObservedAt?: string;
  sessionId?: string;
  workspaceId?: string;
  sessionObserved?: string;
  workspaceObserved?: string;
  lastOutcome?: string;
  reason?: string;
  createdAt: string;
  updatedAt: string;
}

export interface PaseoSession {
  attemptId: string;
  runId: string;
  seq: number;
  workerId: string;
  leaseGeneration: number;
  provider: string;
  model: string;
  mode: string;
  execMode: string;
  workspaceId?: string;
  sessionId?: string;
  correlationKey?: string;
  status: string;
  error?: string;
  /** S3-D cleanup/reconciliation receipt; absent until claimed. */
  recovery?: PaseoRecovery;
  createdAt: string;
  updatedAt: string;
}

export interface PaseoSessionSummary extends PaseoSession {
  /** Current status of the Run that owns this binding. */
  runStatus: string;
}

export interface PaseoSessionList {
  sessions: PaseoSessionSummary[];
  /** Durable ledger head at snapshot time; used as the SSE start cursor. */
  eventCursor: number;
}

export type CheckStatus =
  | "PASS"
  | "FAIL"
  | "SKIPPED"
  | "NOT_APPLICABLE"
  | "NOT_TESTED";

export interface SpecCheck {
  id: string;
  kind: string;
  description: string;
  command: string[];
  expectExit: number;
  timeoutSecs: number;
}

export interface ActionSpec {
  id: string;
  projectId: string;
  actionId: string;
  version: number;
  contentHash: string;
  policyRevision: string;
  goal: string;
  repository: string;
  baseRevision: string;
  allowedPaths: string[];
  acceptanceCriteria: SpecCheck[];
  negativeCases: SpecCheck[];
  riskClass: string;
  qualityProfile: string;
  budget: { maxAttempts: number; timeoutSecs: number };
  executionMode: string;
  provider: string;
  model: string;
  paseoMode: string;
  projectConfigRevision: number;
  repositoryIdentity?: string;
  capabilities: Capability[];
  browserEndpoints?: string[];
  bindingHash?: string;
  createdAt: string;
  frozenAt: string;
}

export interface Candidate {
  id: string;
  projectId: string;
  actionId: string;
  specId: string;
  specHash: string;
  specBindingHash?: string;
  policyRevision: string;
  baseRevision: string;
  commitSha: string;
  treeSha: string;
  contentDigest: string;
  candidateRef: string;
  parentCandidateId?: string;
  repository: string;
  implementerRunId: string;
  implementerAttemptId: string;
  implementerGeneration: number;
  implementerWorkerId: string;
  executionMode: string;
  provider: string;
  model: string;
  paseoMode: string;
  changedPaths: string[];
  riskClass: string;
  state: string;
  createdAt: string;
  frozenAt: string;
  supersededAt?: string;
}

export interface CheckResult {
  id: string;
  kind: string;
  status: CheckStatus;
  command?: string[];
  exitCode: number;
  durationMs: number;
  detail?: string;
}

export interface VerificationEvidence {
  id: string;
  candidateId: string;
  candidateSha: string;
  specId: string;
  specHash: string;
  policyRevision: string;
  authority: string;
  actor: string;
  independent: boolean;
  cleanResource: string;
  oracleDigest: string;
  status: string;
  checks: CheckResult[];
  findings: string[];
  idempotencyKey: string;
  createdAt: string;
}

export interface ReviewFinding {
  id: string;
  severity: string;
  message: string;
}

export interface ReviewEvidence {
  id: string;
  candidateId: string;
  candidateSha: string;
  specId: string;
  specHash: string;
  policyRevision: string;
  authority: string;
  actor: string;
  verdict: string;
  findings: ReviewFinding[];
  diffStat: string;
  diffBytes: number;
  idempotencyKey: string;
  createdAt: string;
}

export interface GateDecision {
  id: string;
  candidateId: string;
  candidateSha: string;
  specId: string;
  specHash: string;
  policyRevision: string;
  status: string;
  evidenceDigest?: string;
  reasons: string[];
  checks: CheckResult[];
  actor: string;
  createdAt: string;
}

export interface CandidateBundle {
  candidate: Candidate;
  spec: ActionSpec;
  verifications: VerificationEvidence[];
  reviews: ReviewEvidence[];
  decision?: GateDecision;
}

export interface Attempt {
  id: string;
  runId: string;
  seq: number;
  workerId: string;
  leaseGeneration: number;
  status: string;
  heartbeats: number;
  result?: Record<string, unknown>;
  error?: string;
  createdAt: string;
  updatedAt: string;
}

export interface Worker {
  id: string;
  version?: string;
  status: string;
  lastHeartbeat?: string;
  lastRunId?: string;
  createdAt: string;
  updatedAt: string;
}

export type CampaignStatus =
  | "pending"
  | "running"
  | "accepted"
  | "repair_required"
  | "blocked"
  | "human_required"
  | "failed"
  | "cancelled";

export interface CampaignTaskPlan {
  key: string;
  role: string;
  cycle: number;
  dependsOn: string[];
  timeoutSecs: number;
  maxExecutions: number;
}

export interface CampaignPlan {
  revision: number;
  maxRepairCycles: number;
  tasks: CampaignTaskPlan[];
  parallelism: number;
  wallClockSeconds: number;
}

export interface Campaign {
  id: string;
  projectId: string;
  actionId: string;
  specId: string;
  specHash: string;
  policyRevision: string;
  planHash: string;
  plan: CampaignPlan;
  status: CampaignStatus;
  terminalReason?: string;
  maxRepairCycles: number;
  repairCycles: number;
  maxAttempts: number;
  maxWallClockSecs: number;
  maxParallel: number;
  correlationId?: string;
  idempotencyKey?: string;
  createdAt: string;
  updatedAt: string;
  startedAt?: string;
  completedAt?: string;
}

export interface CampaignTask {
  id: string;
  campaignId: string;
  key: string;
  role: string;
  cycle: number;
  status: string;
  dependsOn: string[];
  participantId?: string;
  candidateId?: string;
  runId?: string;
  outcome?: string;
  error?: string;
  executionCount: number;
  maxExecutions: number;
  idempotencyKey?: string;
  dispatchOwner?: string;
  dispatchExpiresAt?: string;
  createdAt: string;
  updatedAt: string;
  startedAt?: string;
  completedAt?: string;
}

export interface CampaignBudgetUsage {
  maxAttempts: number;
  attemptsUsed: number;
  maxRepairCycles: number;
  repairCycles: number;
  maxWallClockSecs: number;
  elapsedSecs: number;
  remainingWallClockSecs: number;
  maxParallel: number;
  activeParticipantMax: number;
  providerBudget: string;
}

export interface CampaignProjection {
  campaign: Campaign;
  tasks: CampaignTask[];
  budget: CampaignBudgetUsage;
  candidate?: Candidate;
  decision?: GateDecision;
}

export class ApiError extends Error {
  status: number;
  code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

function newCorrelationId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `corr-${Date.now()}-${Math.floor(Math.random() * 1e9)}`;
}

export function newIdempotencyKey(): string {
  return newCorrelationId();
}

async function request<T>(
  path: string,
  init: RequestInit & { idempotencyKey?: string } = {},
): Promise<{ data: T; replayed: boolean }> {
  const token = getToken();
  const headers = new Headers(init.headers);
  headers.set("Content-Type", "application/json");
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }
  headers.set("X-Correlation-ID", newCorrelationId());
  if (init.idempotencyKey) {
    headers.set("Idempotency-Key", init.idempotencyKey);
  }
  const res = await fetch(path, { ...init, headers });
  if (res.status === 401) {
    clearToken();
    throw new ApiError(401, "unauthorized", "Valid Bearer token required.");
  }
  let body: unknown = null;
  try {
    body = await res.json();
  } catch {
    throw new ApiError(
      res.status,
      "invalid",
      `Unreadable response (${res.status}).`,
    );
  }
  if (!res.ok) {
    const err = body as { error?: { code?: string; message?: string } };
    throw new ApiError(
      res.status,
      err?.error?.code ?? "internal",
      err?.error?.message ?? `Request failed (${res.status}).`,
    );
  }
  return {
    data: body as T,
    replayed: res.headers.get("Idempotent-Replayed") === "true",
  };
}

export const api = {
  health: () => request<Health>("/api/v1/health").then((r) => r.data),
  listProjects: () =>
    request<{ projects: Project[] }>("/api/v1/projects").then(
      (r) => r.data.projects,
    ),
  getProject: (id: string) =>
    request<{ project: Project }>(
      `/api/v1/projects/${encodeURIComponent(id)}`,
    ).then((r) => r.data.project),
  registerProject: (input: {
    id: string;
    name: string;
    repositoryPath: string;
    baseRefs: string[];
    qualityProfile: string;
    workspaceScope: string;
    capabilities: string[];
    browserEndpoints: string[];
    executionMode?: string;
  }) =>
    request<{ project: Project }>("/api/v1/projects", {
      method: "POST",
      body: JSON.stringify(input),
    }).then((r) => r.data.project),
  updateProject: (
    id: string,
    input: {
      name?: string;
      status?: string;
      baseRefs?: string[];
      qualityProfile?: string;
      workspaceScope?: string;
      capabilities?: string[];
      browserEndpoints?: string[];
    },
  ) =>
    request<{ project: Project }>(
      `/api/v1/projects/${encodeURIComponent(id)}`,
      { method: "PATCH", body: JSON.stringify(input) },
    ).then((r) => r.data.project),
  projectRefs: (id: string) =>
    request<{
      repositoryIdentity: string;
      repositoryPath: string;
      baseRefs: Record<string, string>;
    }>(`/api/v1/projects/${encodeURIComponent(id)}/refs`).then((r) => r.data),
  projectEvents: (id: string) =>
    request<{ events: FactoryEvent[] }>(
      `/api/v1/projects/${encodeURIComponent(id)}/events`,
    ).then((r) => r.data.events),
  listActions: (params: { projectId?: string; status?: string } = {}) => {
    const q = new URLSearchParams();
    if (params.projectId) q.set("projectId", params.projectId);
    if (params.status) q.set("status", params.status);
    const suffix = q.toString() ? `?${q.toString()}` : "";
    return request<{ actions: Action[] }>(`/api/v1/actions${suffix}`).then(
      (r) => r.data.actions,
    );
  },
  getAction: (id: string) =>
    request<{ action: Action }>(
      `/api/v1/actions/${encodeURIComponent(id)}`,
    ).then((r) => r.data.action),
  createAction: (
    input: { projectId?: string; title: string; description?: string },
    idempotencyKey: string,
  ) =>
    request<{ action: Action }>("/api/v1/actions", {
      method: "POST",
      body: JSON.stringify(input),
      idempotencyKey,
    }),
  createActionSpec: (input: {
    projectId: string;
    actionId: string;
    goal: string;
    repository?: string;
    baseRevision: string;
    allowedPaths: string[];
    acceptanceCriteria: SpecCheck[];
    negativeCases: SpecCheck[];
    riskClass: string;
    qualityProfile: string;
    budget: { maxAttempts: number; timeoutSecs: number };
    executionMode: string;
    provider: string;
    model: string;
    paseoMode: string;
    capabilities?: string[];
  }) =>
    request<{ spec: ActionSpec }>("/api/v1/action-specs", {
      method: "POST",
      body: JSON.stringify(input),
    }).then((r) => r.data.spec),
  transitionAction: (id: string, to: ActionStatus) =>
    request<{ action: Action }>(
      `/api/v1/actions/${encodeURIComponent(id)}/transition`,
      {
        method: "POST",
        body: JSON.stringify({ to }),
      },
    ).then((r) => r.data.action),
  listActionEvents: (id: string) =>
    request<{ events: FactoryEvent[] }>(
      `/api/v1/actions/${encodeURIComponent(id)}/events`,
    ).then((r) => r.data.events),
  createRun: (
    input: { message: string; steps?: number; stepMs?: number },
    idempotencyKey: string,
  ) =>
    request<{ run: Run }>("/api/v1/runs", {
      method: "POST",
      body: JSON.stringify({
        workloadKind: "synthetic-echo",
        workloadInput: {
          message: input.message,
          ...(input.steps !== undefined ? { steps: input.steps } : {}),
          ...(input.stepMs !== undefined ? { stepMs: input.stepMs } : {}),
        },
      }),
      idempotencyKey,
    }),
  listRuns: (params: { status?: string } = {}) => {
    const q = params.status
      ? `?status=${encodeURIComponent(params.status)}`
      : "";
    return request<{ runs: Run[] }>(`/api/v1/runs${q}`).then(
      (r) => r.data.runs,
    );
  },
  getRun: (id: string) =>
    request<{ run: Run }>(`/api/v1/runs/${encodeURIComponent(id)}`).then(
      (r) => r.data.run,
    ),
  cancelRun: (id: string) =>
    request<{ run: Run }>(`/api/v1/runs/${encodeURIComponent(id)}/cancel`, {
      method: "POST",
      body: "{}",
    }).then((r) => r.data.run),
  listAttempts: (id: string) =>
    request<{ attempts: Attempt[] }>(
      `/api/v1/runs/${encodeURIComponent(id)}/attempts`,
    ).then((r) => r.data.attempts),
  listActionSpecs: (id: string) =>
    request<{ specs: ActionSpec[] }>(
      `/api/v1/actions/${encodeURIComponent(id)}/specs`,
    ).then((r) => r.data.specs),
  listCandidates: (
    params: {
      projectId?: string;
      actionId?: string;
      runId?: string;
      limit?: number;
    } = {},
  ) => {
    const q = new URLSearchParams();
    if (params.projectId) q.set("projectId", params.projectId);
    if (params.actionId) q.set("actionId", params.actionId);
    if (params.runId) q.set("runId", params.runId);
    if (params.limit !== undefined) q.set("limit", String(params.limit));
    const suffix = q.toString() ? `?${q.toString()}` : "";
    return request<{ candidates: Candidate[] }>(
      `/api/v1/candidates${suffix}`,
    ).then((r) => r.data.candidates);
  },
  getCandidateEvidence: (id: string) =>
    request<{ bundle: CandidateBundle }>(
      `/api/v1/candidates/${encodeURIComponent(id)}/evidence`,
    ).then((r) => r.data.bundle),
  evaluateGate: (id: string) =>
    request<{ decision: GateDecision; final: boolean }>(
      `/api/v1/candidates/${encodeURIComponent(id)}/gate`,
      { method: "POST", body: "{}" },
    ).then((r) => r.data),
  listCampaigns: (
    params: { projectId?: string; status?: string; limit?: number } = {},
  ) => {
    const q = new URLSearchParams();
    if (params.projectId) q.set("projectId", params.projectId);
    if (params.status) q.set("status", params.status);
    if (params.limit !== undefined) q.set("limit", String(params.limit));
    const suffix = q.toString() ? `?${q.toString()}` : "";
    return request<{ campaigns: Campaign[] }>(
      `/api/v1/campaigns${suffix}`,
    ).then((r) => r.data.campaigns);
  },
  getCampaign: (id: string) =>
    request<CampaignProjection>(
      `/api/v1/campaigns/${encodeURIComponent(id)}`,
    ).then((r) => r.data),
  createCampaign: (
    input: {
      specId: string;
      maxRepairCycles?: number;
      maxWallClockSecs?: number;
      maxParallel?: number;
    },
    idempotencyKey: string,
  ) =>
    request<CampaignProjection>("/api/v1/campaigns", {
      method: "POST",
      body: JSON.stringify(input),
      idempotencyKey,
    }),
  startCampaign: (id: string) =>
    request<CampaignProjection>(
      `/api/v1/campaigns/${encodeURIComponent(id)}/start`,
      { method: "POST", body: "{}" },
    ).then((r) => r.data),
  cancelCampaign: (id: string) =>
    request<CampaignProjection>(
      `/api/v1/campaigns/${encodeURIComponent(id)}/cancel`,
      { method: "POST", body: "{}" },
    ).then((r) => r.data),
  listCampaignEvents: (id: string) =>
    request<{ events: FactoryEvent[] }>(
      `/api/v1/campaigns/${encodeURIComponent(id)}/events`,
    ).then((r) => r.data.events),
  listRunEvents: (id: string) =>
    request<{ events: FactoryEvent[] }>(
      `/api/v1/runs/${encodeURIComponent(id)}/events`,
    ).then((r) => r.data.events),
  listSessions: (id: string) =>
    request<{ sessions: PaseoSession[] }>(
      `/api/v1/runs/${encodeURIComponent(id)}/sessions`,
    ).then((r) => r.data.sessions),
  listAllSessions: (
    params: { status?: string; provider?: string; limit?: number } = {},
  ) => {
    const q = new URLSearchParams();
    if (params.status) q.set("status", params.status);
    if (params.provider) q.set("provider", params.provider);
    if (params.limit !== undefined) q.set("limit", String(params.limit));
    const suffix = q.toString() ? `?${q.toString()}` : "";
    return request<PaseoSessionList>(`/api/v1/sessions${suffix}`).then(
      (r) => r.data,
    );
  },
  listWorkers: () =>
    request<{ workers: Worker[] }>(`/api/v1/workers`).then(
      (r) => r.data.workers,
    ),
  getWorker: (id: string) =>
    request<{ worker: Worker }>(
      `/api/v1/workers/${encodeURIComponent(id)}`,
    ).then((r) => r.data.worker),
  listEvents: (after: number, limit = 100, signal?: AbortSignal) =>
    request<{ events: FactoryEvent[]; nextAfter: number }>(
      `/api/v1/events?after=${after}&limit=${limit}`,
      { signal },
    ).then((r) => r.data),
};
