// Factory Office API client. All state is server-authoritative: the UI
// renders Core snapshots and the durable event ledger, never invented data.

export type ActionStatus =
  | "pending"
  | "in_progress"
  | "completed"
  | "cancelled";

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
  createdAt: string;
  updatedAt: string;
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
    request<{ projects: { id: string; name: string }[] }>(
      "/api/v1/projects",
    ).then((r) => r.data.projects),
  listActions: (params: { status?: string } = {}) => {
    const q = params.status
      ? `?status=${encodeURIComponent(params.status)}`
      : "";
    return request<{ actions: Action[] }>(`/api/v1/actions${q}`).then(
      (r) => r.data.actions,
    );
  },
  getAction: (id: string) =>
    request<{ action: Action }>(
      `/api/v1/actions/${encodeURIComponent(id)}`,
    ).then((r) => r.data.action),
  createAction: (
    input: { title: string; description?: string },
    idempotencyKey: string,
  ) =>
    request<{ action: Action }>("/api/v1/actions", {
      method: "POST",
      body: JSON.stringify(input),
      idempotencyKey,
    }),
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
  listRunEvents: (id: string) =>
    request<{ events: FactoryEvent[] }>(
      `/api/v1/runs/${encodeURIComponent(id)}/events`,
    ).then((r) => r.data.events),
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
