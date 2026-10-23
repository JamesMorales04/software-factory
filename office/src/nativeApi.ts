// Rust Core API client for the native Office view. The legacy client remains
// for retained views until they are migrated.

import { getToken } from "./api";

const BASE = "/native-api";

export interface NativeProject {
  id: string;
  repository: string;
  base_commit: string;
  created_at: string;
}

export interface NativeAction {
  id: string;
  project_id: string;
  objective: string;
  provider: string;
  model: string;
  risk: string;
  status: string;
  spec_sha256: string;
}

export interface NativeCampaign {
  id: string;
  action_id: string;
  status: string;
}

export interface NativeCandidate {
  id: string;
  attempt_id: string;
  action_spec_sha256: string;
  commit_sha: string;
  tree_sha: string;
  implementer_id: string;
}

export interface NativeGate {
  candidate_id: string;
  outcome: string;
  reasons: string[];
}

export class NativeApiError extends Error {}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = getToken();
  if (!token) throw new NativeApiError("Office is locked.");
  const headers = new Headers(init.headers);
  headers.set("authorization", `Bearer ${token}`);
  if (init.body) headers.set("content-type", "application/json");
  const response = await fetch(`${BASE}${path}`, { ...init, headers });
  if (!response.ok) {
    const body = await response.text();
    throw new NativeApiError(`${response.status}: ${body}`);
  }
  return (await response.json()) as T;
}

export function listProjects() {
  return request<NativeProject[]>("/projects");
}

export function registerProject(repository: string, key: string) {
  return request<NativeProject>("/projects", {
    method: "POST",
    body: JSON.stringify({ repository, idempotency_key: key }),
  });
}

export function listActions() {
  return request<NativeAction[]>("/actions");
}

export function createAction(input: {
  projectId: string;
  objective: string;
  provider: string;
  model: string;
  key: string;
  allowedPaths: string[];
}) {
  return request<NativeAction>("/actions", {
    method: "POST",
    body: JSON.stringify({
      project_id: input.projectId,
      objective: input.objective,
      provider: input.provider,
      model: input.model,
      idempotency_key: input.key,
      allowed_paths: input.allowedPaths,
      checks: [
        {
          id: "content",
          kind: "functional",
          description: "fixture content",
          command: ["grep", "-q", "fixture candidate", "src/fixture.txt"],
          expectExit: 0,
          timeoutSecs: 30,
        },
      ],
    }),
  });
}

export function listCampaigns() {
  return request<NativeCampaign[]>("/campaigns");
}

export function createCampaign(actionId: string, key: string) {
  return request<NativeCampaign>("/campaigns", {
    method: "POST",
    body: JSON.stringify({ action_id: actionId, idempotency_key: key }),
  });
}

export function authorizeFixtureCampaign(campaignId: string) {
  return request<unknown>(`/campaigns/${campaignId}/authorize`, {
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
  });
}

export function createRun(campaignId: string) {
  return request<{ id: string }>(`/campaigns/${campaignId}/runs`, {
    method: "POST",
  });
}

export function createAttempt(runId: string) {
  return request<{ id: string }>(`/runs/${runId}/attempts`, {
    method: "POST",
  });
}

export function listCandidates() {
  return request<NativeCandidate[]>("/candidates");
}

export async function getGate(candidateId: string): Promise<NativeGate | null> {
  try {
    return await request<NativeGate>(`/quality/candidates/${candidateId}/gate`);
  } catch (error) {
    if (error instanceof NativeApiError && error.message.startsWith("404:")) {
      return null;
    }
    throw error;
  }
}
