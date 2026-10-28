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
  identity?: string;
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

export interface ProviderTurnBudget {
  reserved_turns: number;
  ceiling_micros: number;
}

export interface WorkerSummary {
  worker_id: string;
  updated_at: string;
}

export interface ProviderSession {
  session_record_id: string;
  attempt_id: string;
  worker_id: string;
  generation: number;
  provider: string;
  model: string;
  provider_mode: string;
  state: string;
  provider_session_id: string;
  provider_directory: string;
  turn_id: string;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost_micros: number;
  usage_known: boolean;
  cleanup?: string;
  cleanup_receipt: string;
  error: string;
  created_at: string;
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

export interface LeadTask {
  task_id: string;
  campaign_id: string;
  request_id: string;
  unit_id: string;
  sequence: number;
  status: string;
  base_commit: string;
  depends_on: string[];
  run_id: string | null;
  attempt_id: string | null;
  candidate_id: string | null;
  candidate_commit: string | null;
  candidate_tree: string | null;
  updated_at: string;
}

export type ParticipantRole =
  | "IMPLEMENTER"
  | "LEAD"
  | "VERIFIER"
  | "REVIEWER"
  | "WORKER";

export type ParticipantObservation =
  | "CURRENT"
  | "STALE"
  | "FINAL"
  | "NOT_EXPOSED"
  | "UNKNOWN";

export interface ParticipantRoute {
  provider: string;
  model: string;
  mode: string | null;
}

export interface ParticipantUsage {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost_micros: number;
  usage_known: boolean;
}

export interface ParticipantSession {
  session_record_id: string;
  attempt_id: string;
  worker_id: string;
  generation: number;
  provider: string;
  model: string;
  provider_mode: string;
  state: string;
  provider_session_id: string | null;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost_micros: number;
  usage_known: boolean;
  cleanup: string;
  cleanup_receipt: string | null;
  error: string | null;
  created_at: string;
  updated_at: string;
}

export interface ParticipantEvidence {
  candidate_id: string | null;
  candidate_commit: string | null;
  candidate_tree: string | null;
  gate_outcome: string | null;
  gate_decided_at: string | null;
  verifier_receipt_id: string | null;
  reviewer_receipt_id: string | null;
}

export interface ParticipantRecord {
  participant_id: string;
  role: ParticipantRole;
  identity_kind: string;
  identity_id: string;
  principal_id: string | null;
  project_id: string | null;
  action_id: string | null;
  campaign_id: string | null;
  request_id: string | null;
  work_unit_id: string | null;
  run_id: string | null;
  attempt_id: string | null;
  attempt_generation: number | null;
  worker_id: string | null;
  requested_route: ParticipantRoute | null;
  observed_route: ParticipantRoute | null;
  route_provenance: string;
  session: ParticipantSession | null;
  status: string;
  status_source: string;
  lease_state: string;
  observation: ParticipantObservation;
  failure_reason: string | null;
  usage: ParticipantUsage | null;
  usage_provenance: string;
  evidence: ParticipantEvidence | null;
  model_call_count: number | null;
  model_call_state: string | null;
  observation_note: string;
  recorded_at: string;
  last_observed_at: string;
}

export interface ParticipantPageInfo {
  limit: number;
  offset: number;
  returned: number;
  has_more: boolean;
  order: string;
  as_of: string;
}

export interface ParticipantSource {
  role: ParticipantRole;
  source: string;
  note: string;
}

export interface ParticipantPage {
  participants: ParticipantRecord[];
  page: ParticipantPageInfo;
  sources: ParticipantSource[];
}

export interface ParticipantFilters {
  role?: string;
  provider?: string;
  status?: string;
  campaignId?: string;
  projectId?: string;
  participantId?: string;
  limit?: number;
  offset?: number;
}

export interface ProviderSessionContext {
  participant_id: string;
  session_record_id: string;
  attempt_id: string;
  run_id: string;
  campaign_id: string;
  worker_id: string;
  generation: number;
  provider: string;
  model: string;
  provider_mode: string;
  state: string;
  lease_state: string;
  cleanup: string;
  updated_at: string;
}

export interface ProviderToolEventView {
  sequence: number;
  call_ref: string;
  tool: string;
  tool_digest: string;
  decision: string;
  detail: string | null;
  detail_state: string;
  recorded_at: string;
  provenance: string;
}

export interface ProviderToolEventProjection {
  session: ProviderSessionContext;
  events: ProviderToolEventView[];
  after_sequence: number;
  limit: number;
  returned: number;
  next_sequence: number | null;
  has_more: boolean;
  discarded: number;
  source: string;
  note: string;
}

export interface CandidateDiffSummary {
  candidate_id: string;
  commit_sha: string;
  tree_sha: string;
  base_commit: string;
  changed_paths: string[];
  changed_paths_truncated: boolean;
  diff_stat: string;
  source: string;
}

export interface CampaignDetail {
  campaign: Campaign;
  authorization: CampaignAuthorization | null;
  tasks: LeadTask[];
  runs: Run[];
  attempts: Attempt[];
  candidates: Candidate[];
  events: FactoryEvent[];
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
  provider: string;
  model: string;
  provider_mode: string;
  delegation_id: string;
  max_sessions: number;
  max_turns: number;
  max_input_tokens: number;
  max_output_tokens: number;
  max_cost_micros: number;
  deadline_secs: number;
  allowed_tools: string[];
  created_at: string;
}

export type ReceiptResult =
  | "PASS"
  | "FAIL"
  | "CHANGES_REQUESTED"
  | "NOT_TESTED";
export type EvidenceKind = "EXECUTED" | "MOCKED" | "UNKNOWN";
export type QualityRole = "verifier" | "reviewer";

export type ReviewMechanism = "DETERMINISTIC" | "MODEL";
export type FindingSeverity = "blocking" | "warning" | "info";
export type FindingCategory =
  | "functional"
  | "architecture"
  | "security"
  | "testing"
  | "scope"
  | "other";

export interface ReviewFinding {
  id: string;
  severity: FindingSeverity;
  category: FindingCategory;
  message: string;
  evidence_refs: string[];
}

export interface QualityReceipt {
  receipt_id: string;
  candidate_id: string;
  role: QualityRole;
  principal_id: string;
  action_spec_sha256: string;
  commit_sha: string;
  tree_sha: string;
  result: ReceiptResult;
  evidence: EvidenceKind;
  findings: ReviewFinding[];
  mechanism: ReviewMechanism;
  model_provider: string;
  model_name: string;
  model_reservation_id: string;
  model_request_sha256: string;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost_micros: number;
  usage_known: boolean;
  received_at: string;
}

export type ModelCallRole = "lead" | "semantic_reviewer" | "evaluation";

export interface ModelCallRecord {
  reservation_id: string;
  role: ModelCallRole;
  principal_id: string;
  project_id: string;
  objective_id: string;
  scope_id: string;
  provider: string;
  model: string;
  purpose: string;
  request_sha256: string;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost_micros: number;
  max_cost_micros: number;
  usage_known: boolean;
  state: string;
  error: string;
  created_at: string;
  updated_at: string;
}

export interface ObservabilityStatus {
  enabled: boolean;
  cursor: number;
  exported: number;
  dropped: number;
  failed_attempts: number;
  pending: number;
  last_error: string | null;
}

export interface Health {
  healthy: boolean;
  version: string;
}

export interface OverviewCounts {
  projects: number;
  actions: number;
  campaigns: number;
  runs: number;
  attempts: number;
  candidates: number;
  workers: number;
  lead_requests: number;
  authorized_campaigns: number;
  campaigns_awaiting_authorization: number;
  active_agents: number;
  unresolved_sessions: number;
  unresolved_model_calls: number;
  pending_decisions: number;
  blocked_signals: number;
}

export interface OverviewBudget {
  measured_session_cost_micros: number;
  measured_session_tokens: number;
  measured_model_cost_micros: number;
  measured_model_tokens: number;
  unmeasured_sessions: number;
  unmeasured_model_calls: number;
  unmeasured_bound_cost_micros: number;
  model_calls_sampled: number;
  note: string;
}

export interface OverviewRuntime {
  core_healthy: boolean;
  workers: number;
  active_agents: number;
  unresolved_sessions: number;
  unresolved_model_calls: number;
  observability: ObservabilityStatus;
}

export interface RecentAcceptedCandidate {
  candidate_id: string;
  campaign_id: string;
  commit_sha: string;
  decided_at: string;
  outcome: GateOutcome;
}

export interface DecisionAction {
  kind: "resolve_model_call";
  model_call_id: string;
  label: string;
}

export interface DecisionItem {
  id: string;
  kind: string;
  severity: "critical" | "attention" | "info";
  actionable: boolean;
  title: string;
  detail: string;
  target: string;
  subject_id: string;
  updated_at: string;
  action?: DecisionAction;
}

export interface OfficeOverview {
  as_of: string;
  source: string;
  counts: OverviewCounts;
  actions_by_status: Record<string, number>;
  campaigns_by_status: Record<string, number>;
  runs_by_status: Record<string, number>;
  gate_outcomes: Record<string, number>;
  budget: OverviewBudget;
  runtime: OverviewRuntime;
  recent_accepted: RecentAcceptedCandidate[];
  recent_decisions: DecisionItem[];
  recent_events: FactoryEvent[];
}

export interface DecisionList {
  items: DecisionItem[];
  as_of: string;
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

export interface ProjectConfig {
  projectId: string;
  revision: number;
  configHash: string;
  repository: string;
  baseCommit: string;
  allowedPaths: string[];
  capabilities: string[];
  qualityProfile: string;
  active: boolean;
  updatedAt: string;
}

export interface UpdateProjectConfig {
  expectedRevision: number;
  allowedPaths: string[];
  capabilities: string[];
  qualityProfile: string;
  active: boolean;
}

export interface ProviderRoute {
  provider: string;
  model: string;
  mode: string;
}

export interface StandingDelegation {
  id: string;
  principalId: string;
  auditId: string;
  projectId: string;
  revision: number;
  contentHash: string;
  objectiveClasses: string[];
  allowedPaths: string[];
  capabilities: string[];
  riskClasses: string[];
  autonomousStart: boolean;
  autonomousRiskClasses: string[];
  route: ProviderRoute;
  maxAttempts: number;
  maxTimeoutSecs: number;
  maxRepairCycles: number;
  maxWallClockSecs: number;
  maxParallel: number;
  maxSessions: number;
  maxRequests: number;
  maxCumulativeSessions: number;
  maxCumulativeAttempts: number;
  expiresAt: string;
  revokedAt?: string;
}

export interface LeadRequestRecord {
  id: string;
  project_id: string;
  action_id: string;
  delegation_id: string;
  objective: string;
  request_hash: string;
  project_config_hash: string;
  state: string;
  assessment_mechanism: string | null;
  failure_reason: string | null;
  campaign_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface LeadAssessmentRecord {
  id: string;
  request_id: string;
  mechanism: string;
  assessment_json: string;
  schema_sha256: string;
  created_at: string;
}

export interface LeadProposalRecord {
  id: string;
  request_id: string;
  assessment_id: string;
  proposal_json: string;
  proposal_hash: string;
  created_at: string;
}

export interface FrozenLeadPlanRecord {
  id: string;
  request_id: string;
  proposal_id: string;
  plan_hash: string;
  config_hash: string;
  delegation_hash: string;
  created_at: string;
}

export interface LeadRequestProjection {
  request: LeadRequestRecord;
  assessment: LeadAssessmentRecord | null;
  proposal: LeadProposalRecord | null;
  plan: FrozenLeadPlanRecord | null;
  model_calls: ModelCallRecord[];
}

export interface EffortEstimateView {
  status: string;
  unit: string;
  minPersonHours: number;
  maxPersonHours: number;
  assumptions: string[];
  unknownReason: string;
}

export interface LeadAssessmentView {
  schemaRevision: number;
  projectId: string;
  actionId: string;
  requestId: string;
  evidenceRefs: string[];
  intent: string;
  complexity: string;
  effortEstimate: EffortEstimateView;
  knowledgeGaps: string[];
  changeImpact: string[];
  validationNeeds: string[];
  assumptions: string[];
  uncertainty: string[];
  suggestedNextAction: string;
}

export interface LeadWorkUnitView {
  id: string;
  kind: string;
  description: string;
  dependsOn: string[];
  allowedPaths: string[];
  capabilities: string[];
  acceptanceCriteria: SpecCheck[];
  negativeCases: SpecCheck[];
}

export interface LeadPlanProposalView {
  projectId: string;
  actionId: string;
  baseRevision: string;
  projectConfigHash: string;
  objective: string;
  class: string;
  assumptions: string[];
  risks: string[];
  acceptanceCriteria: SpecCheck[];
  negativeCases: SpecCheck[];
  qualityProfile: string;
  riskClass: string;
  budget: { maxAttempts: number; timeoutSecs: number };
  route: ProviderRoute;
  maxRepairCycles: number;
  maxWallClockSecs: number;
  maxParallel: number;
  maxSessions: number;
  workUnits: LeadWorkUnitView[];
}

export interface NewLeadRequest {
  project_id: string;
  action_id: string;
  delegation_id: string;
  objective: string;
  project_config_hash: string;
  idempotency_key: string;
}

export interface NewCampaignRequest {
  action_id: string;
  idempotency_key: string;
}

export interface NewCampaignAuthorization {
  authorization_id: string;
  risk_class: string;
  execution_mode: string;
  actor: string;
  rationale: string;
  authorized_paths: string[];
  max_attempts: number;
  provider: string;
  model: string;
  provider_mode: string;
  delegation_id: string;
  max_sessions: number;
  max_turns: number;
  max_input_tokens: number;
  max_output_tokens: number;
  max_cost_micros: number;
  deadline_secs: number;
  allowed_tools: string[];
}

export interface StartLeadCampaignResult {
  campaign: Campaign;
  run: Run;
  attempt: Attempt;
}

export interface ProviderView {
  provider: string;
  kind: string;
  displayName: string;
  defaultModel: string;
  defaultBaseUrl: string;
  authModes: string[];
  chatgptOauth: string;
  status: string;
  note: string;
  configured: boolean;
  enabled: boolean;
  isDefault: boolean;
  model: string;
  baseUrl: string;
  credentialFingerprint: string;
  revision: number;
  updatedAt: string;
  maxTurns: number;
  maxInputTokens: number;
  maxOutputTokens: number;
  maxCostMicros: number;
  inputMicrosPerMillion: number;
  outputMicrosPerMillion: number;
}

export interface UpdateProviderRequest {
  model: string;
  baseUrl: string;
  credential?: string;
  enabled: boolean;
  isDefault: boolean;
  maxTurns: number;
  maxInputTokens: number;
  maxOutputTokens: number;
  maxCostMicros: number;
  inputMicrosPerMillion: number;
  outputMicrosPerMillion: number;
}

export interface ProviderTestResult {
  provider: string;
  status: "AUTHENTICATED" | "DENIED" | "UNREACHABLE" | "UNSUPPORTED";
  models: string[];
  model: string;
  modelAvailable: boolean;
  detail: string;
}

export interface FactorySettings {
  semanticReviewEnabled: boolean;
  semanticReviewMaxOutputTokens: number;
  updatedAt: string;
}

export interface UpdateFactorySettings {
  semanticReviewEnabled: boolean;
  semanticReviewMaxOutputTokens: number;
}

export interface ProviderAudit {
  auditId: string;
  provider: string;
  revision: number;
  action: string;
  actor: string;
  detail: string;
  createdAt: string;
}

export interface NewDelegationRequest {
  delegation: {
    id: string;
    principalId: string;
    auditId: string;
    projectId: string;
    revision: number;
    contentHash: string;
    objectiveClasses: string[];
    allowedPaths: string[];
    capabilities: string[];
    riskClasses: string[];
    autonomousStart: boolean;
    autonomousRiskClasses: string[];
    route: ProviderRoute;
    maxAttempts: number;
    maxTimeoutSecs: number;
    maxRepairCycles: number;
    maxWallClockSecs: number;
    maxParallel: number;
    maxSessions: number;
    maxRequests: number;
    maxCumulativeSessions: number;
    maxCumulativeAttempts: number;
    expiresAt: string;
  };
  idempotency_key: string;
}

export interface DelegationDetail {
  delegation: StandingDelegation;
  usage: {
    sessions: number;
    attempts: number;
    requests: number;
  };
}

export interface SetupStatus {
  required: boolean;
  version: string;
}

export interface DeviceSession {
  id: string;
  name: string;
  createdAt: string | number;
  lastActivityAt: string | number;
  expiresAt: string | number;
}

export interface AuthSession {
  authenticated: boolean;
  status?: string;
  bootstrapAvailable?: boolean;
  csrfToken?: string;
  device?: DeviceSession;
}

export interface AccessOverview {
  local: { status: string };
  tailscale: {
    enabled: boolean;
    status: string;
    httpsEndpoint: string | null;
    error: string | null;
  };
}

export interface DeviceSessionView extends DeviceSession {
  status: "PENDING" | "CONNECTED" | "CANCELLED" | string;
  current?: boolean;
  verificationCode?: string;
}

export interface DeviceChallenge {
  challengeId: string;
  verificationCode: string;
  expiresAt: string | number;
}

export interface DeviceClaim {
  deviceId: string;
  verificationCode: string;
  status:
    | "PENDING"
    | "APPROVED"
    | "REJECTED"
    | "CANCELLED"
    | "EXPIRED"
    | string;
  expiresAt: string | number;
  pollToken: string;
}

export interface DeviceClaimPoll {
  status:
    | "PENDING"
    | "APPROVED"
    | "REJECTED"
    | "CANCELLED"
    | "EXPIRED"
    | string;
  verificationCode?: string;
  expiresAt?: string | number;
}

export interface SecurityAuditEntry {
  action: string;
  actor: string;
  deviceName: string | null;
  occurredAt: string | number;
}

let csrfToken: string | null = null;

export async function setupStatus(): Promise<SetupStatus> {
  const response = await fetch(`${BASE}/setup/status`);
  if (!response.ok) {
    throw new CoreApiError(response.status, "Unable to read setup status.");
  }
  return (await response.json()) as SetupStatus;
}

export async function bootstrapOwner(
  code: string,
  deviceName?: string,
): Promise<AuthSession> {
  const response = await fetch(`${BASE}/auth/bootstrap`, {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ code, ...(deviceName ? { deviceName } : {}) }),
  });
  if (!response.ok) {
    const text = await response.text();
    throw new CoreApiError(
      response.status,
      messageFromBody(response.status, text),
    );
  }
  const body = (await response.json()) as AuthSession;
  setAuthSession(body);
  return body;
}

const BASE = "/core-api";

function setAuthSession(session: AuthSession): void {
  csrfToken = session.csrfToken ?? readCsrfCookie();
}

function readCsrfCookie(): string | null {
  if (typeof document === "undefined") return null;
  const item = document.cookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("factory_csrf="));
  if (!item) return null;
  try {
    return decodeURIComponent(item.slice("factory_csrf=".length));
  } catch {
    return null;
  }
}

export async function getAuthSession(): Promise<AuthSession> {
  const response = await fetch(`${BASE}/auth/session`, {
    credentials: "same-origin",
  });
  if (!response.ok) {
    const text = await response.text();
    throw new CoreApiError(
      response.status,
      messageFromBody(response.status, text),
    );
  }
  const session = (await response.json()) as AuthSession;
  setAuthSession(session);
  return session;
}

export async function logoutCurrentDevice(): Promise<void> {
  await authPost("/auth/logout");
  csrfToken = null;
}

export async function logoutAllDevices(): Promise<void> {
  await authPost("/auth/logout-all");
  csrfToken = null;
}

async function accessRequest<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.body !== undefined) headers.set("content-type", "application/json");
  const method = (init.method ?? "GET").toUpperCase();
  if (method !== "GET" && method !== "HEAD") {
    const token = csrfToken ?? readCsrfCookie();
    if (token) headers.set("x-factory-csrf", token);
  }
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    credentials: "same-origin",
    headers,
  });
  if (!response.ok) {
    const text = await response.text();
    throw new CoreApiError(
      response.status,
      messageFromBody(response.status, text),
    );
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

async function authPost(path: string): Promise<void> {
  const headers = new Headers({ "content-type": "application/json" });
  const token = csrfToken ?? readCsrfCookie();
  if (token) headers.set("x-factory-csrf", token);
  const response = await fetch(`${BASE}${path}`, {
    method: "POST",
    credentials: "same-origin",
    headers,
    body: "{}",
  });
  if (!response.ok) {
    const text = await response.text();
    throw new CoreApiError(
      response.status,
      messageFromBody(response.status, text),
    );
  }
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
  const headers = new Headers(init.headers);
  if (init.body !== undefined) {
    headers.set("content-type", "application/json");
  }
  const method = (init.method ?? "GET").toUpperCase();
  if (method !== "GET" && method !== "HEAD") {
    const token = csrfToken ?? readCsrfCookie();
    if (token) headers.set("x-factory-csrf", token);
  }
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    credentials: "same-origin",
    headers,
  });
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

export interface GitHubConnection {
  configured: boolean;
  enabled: boolean;
  login: string | null;
  scopes: string[];
  scopes_reported: boolean;
  credential_fingerprint: string;
  revision: number;
  verified_at: string | null;
  revoked_at: string | null;
  updated_at: string | null;
  detail: string;
}

export interface GitHubConnectionAuditEntry {
  audit_id: string;
  revision: number;
  action: string;
  actor: string;
  detail: string;
  created_at: string;
}

export interface GitHubConnectionState {
  connection: GitHubConnection;
  audit: GitHubConnectionAuditEntry[];
}

export interface GitHubRepository {
  full_name: string;
  private: boolean;
  default_branch: string;
  html_url: string;
  pushed_at: string | null;
}

export interface GitHubRepositoryList {
  repositories: GitHubRepository[];
  fetched_at: string;
  rate_limited: boolean;
  next_allowed_at: string | null;
  detail: string;
}

export interface ProjectGitHubBinding {
  project_id: string;
  owner: string;
  repo: string;
  repo_full_name: string;
  default_branch: string;
  bound_at: string;
  updated_at: string;
}

export interface GitHubSyncState {
  project_id: string;
  repo_full_name: string;
  status:
    | "NEVER_SYNCED"
    | "OK"
    | "STALE"
    | "RATE_LIMITED"
    | "DENIED"
    | "NOT_FOUND"
    | "ERROR"
    | "REVOKED";
  detail: string;
  etag: string | null;
  last_synced_at: string | null;
  last_attempt_at: string | null;
  next_allowed_at: string | null;
  last_status_code: number | null;
  fetched: number;
  created: number;
  updated: number;
  removed: number;
  total_known: number;
  truncated: boolean;
  stale: boolean;
  updated_at: string;
}

export interface GitHubLabel {
  name: string;
  color: string | null;
}

export interface GitHubIssueSummary {
  number: number;
  title: string;
  state: "open" | "closed";
  state_reason: string | null;
  labels: GitHubLabel[];
  author: string | null;
  html_url: string;
  comments: number;
  created_at: string;
  updated_at: string;
  closed_at: string | null;
  synced_at: string;
}

export interface GitHubIssueDetail extends GitHubIssueSummary {
  body: string;
  body_truncated: boolean;
}

export interface GitHubIssueList {
  items: GitHubIssueSummary[];
  total: number;
  limit: number;
  offset: number;
  synced_at: string | null;
  stale: boolean;
  sync_status: "UNBOUND" | GitHubSyncState["status"];
  last_error: string | null;
  next_allowed_at: string | null;
  truncated: boolean;
  binding: ProjectGitHubBinding | null;
}

export interface GitHubIssueFilters {
  state?: "all" | "open" | "closed";
  label?: string;
  query?: string;
  limit?: number;
  offset?: number;
}

export interface ProjectGitHubBindingResult {
  binding: ProjectGitHubBinding;
  sync_state: GitHubSyncState | null;
}

export const coreApi = {
  getAccessOverview: () => accessRequest<AccessOverview>("/auth/access"),
  createDeviceChallenge: () =>
    accessRequest<DeviceChallenge>("/auth/device-challenges", {
      method: "POST",
      body: JSON.stringify({}),
    }),
  cancelDeviceChallenge: (challengeId: string) =>
    accessRequest<DeviceSessionView>(
      `/auth/device-challenges/${encodeURIComponent(challengeId)}/cancel`,
      { method: "POST", body: JSON.stringify({}) },
    ),
  listDeviceSessions: () =>
    accessRequest<{ devices: DeviceSessionView[] }>("/auth/devices"),
  getSecurityAudit: () =>
    accessRequest<{ entries: SecurityAuditEntry[] }>("/auth/devices/audit"),
  approveDevice: (deviceId: string, verificationCode: string) =>
    accessRequest<DeviceSessionView>(
      `/auth/devices/${encodeURIComponent(deviceId)}/approve`,
      { method: "POST", body: JSON.stringify({ verificationCode }) },
    ),
  rejectDevice: (deviceId: string) =>
    accessRequest<DeviceSessionView>(
      `/auth/devices/${encodeURIComponent(deviceId)}/reject`,
      { method: "POST", body: JSON.stringify({}) },
    ),
  revokeDevice: (deviceId: string) =>
    accessRequest<void>(`/auth/devices/${encodeURIComponent(deviceId)}`, {
      method: "DELETE",
    }),
  claimDevice: (challengeId: string, deviceName: string) =>
    accessRequest<DeviceClaim>(
      `/auth/device-claims/${encodeURIComponent(challengeId)}`,
      {
        method: "POST",
        body: JSON.stringify({ deviceName }),
      },
    ),
  pollDeviceClaim: (challengeId: string, pollToken: string) =>
    accessRequest<DeviceClaimPoll>(
      `/auth/device-claims/${encodeURIComponent(challengeId)}/poll`,
      { method: "POST", body: JSON.stringify({ pollToken }) },
    ),
  health: () => request<Health>("/health"),
  listProjects: () => request<Project[]>("/projects"),
  listProviders: () =>
    request<{ providers: ProviderView[] }>("/settings/providers"),
  getRuntimeSettings: () => request<FactorySettings>("/settings/runtime"),
  updateRuntimeSettings: (input: UpdateFactorySettings) =>
    request<FactorySettings>("/settings/runtime", {
      method: "PUT",
      body: JSON.stringify(input),
    }),
  updateProvider: (provider: string, input: UpdateProviderRequest) =>
    request<ProviderView>(
      `/settings/providers/${encodeURIComponent(provider)}`,
      { method: "PUT", body: JSON.stringify(input) },
    ),
  testProvider: (provider: string) =>
    request<ProviderTestResult>(
      `/settings/providers/${encodeURIComponent(provider)}/test`,
      { method: "POST" },
    ),
  revokeProviderCredential: (provider: string) =>
    request<ProviderView>(
      `/settings/providers/${encodeURIComponent(provider)}/credential`,
      { method: "DELETE" },
    ),
  providerAudit: (provider: string) =>
    request<{ audit: ProviderAudit[] }>(
      `/settings/providers/${encodeURIComponent(provider)}/audit`,
    ),
  createDelegation: (input: NewDelegationRequest) =>
    request<StandingDelegation>("/delegations", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  getDelegation: (id: string) =>
    request<DelegationDetail>(`/delegations/${encodeURIComponent(id)}`),
  revokeDelegation: (id: string) =>
    request<StandingDelegation>(`/delegations/${encodeURIComponent(id)}`, {
      method: "POST",
      body: JSON.stringify({}),
    }),
  getProjectConfig: (projectId: string) =>
    request<ProjectConfig>(`/projects/${encodeURIComponent(projectId)}/config`),
  updateProjectConfig: (projectId: string, input: UpdateProjectConfig) =>
    request<ProjectConfig>(
      `/projects/${encodeURIComponent(projectId)}/config`,
      { method: "PUT", body: JSON.stringify(input) },
    ),
  listDelegations: () => request<StandingDelegation[]>("/delegations"),
  listLeadRequests: () => request<LeadRequestRecord[]>("/lead/requests"),
  getLeadRequest: (id: string) =>
    request<LeadRequestProjection>(`/lead/requests/${encodeURIComponent(id)}`),
  createLeadRequest: (input: NewLeadRequest) =>
    request<LeadRequestRecord>("/lead/requests", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  assessLeadRequest: (id: string) =>
    request<Record<string, unknown>>(
      `/lead/requests/${encodeURIComponent(id)}/assess`,
      { method: "POST" },
    ),
  freezeLeadRequest: (id: string) =>
    request<Record<string, unknown>>(
      `/lead/requests/${encodeURIComponent(id)}/freeze`,
      { method: "POST" },
    ),
  startLeadRequest: (id: string, authorization?: NewCampaignAuthorization) =>
    request<StartLeadCampaignResult>(
      `/lead/requests/${encodeURIComponent(id)}/start`,
      {
        method: "POST",
        ...(authorization === undefined
          ? {}
          : { body: JSON.stringify(authorization) }),
      },
    ),
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
  getCampaignDetail: (id: string) =>
    request<CampaignDetail>(`/campaigns/${encodeURIComponent(id)}`),
  getCandidateDiff: (campaignId: string, candidateId: string) =>
    request<CandidateDiffSummary>(
      `/campaigns/${encodeURIComponent(campaignId)}/candidates/${encodeURIComponent(candidateId)}/diff`,
    ),
  createCampaign: (input: NewCampaignRequest) =>
    request<Campaign>("/campaigns", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  cancelCampaign: (campaignId: string, reason: string) =>
    request<Campaign>(`/campaigns/${encodeURIComponent(campaignId)}/cancel`, {
      method: "POST",
      body: JSON.stringify({ reason }),
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
  listProviderSessions: () => request<ProviderSession[]>("/provider/sessions"),
  listModelCalls: (limit = 100) =>
    request<ModelCallRecord[]>(`/model/calls?limit=${limit}`),
  listUnresolvedModelCalls: () =>
    request<ModelCallRecord[]>("/model/calls/unresolved"),
  getProviderTurnBudget: () =>
    request<ProviderTurnBudget>("/provider/turn_reservations/budget"),
  getObservability: () => request<ObservabilityStatus>("/observability"),
  getOverview: () => request<OfficeOverview>("/office/overview"),
  getGitHubConnection: () =>
    request<GitHubConnectionState>("/integrations/github"),
  updateGitHubConnection: (token: string) =>
    request<GitHubConnectionState>("/integrations/github", {
      method: "PUT",
      body: JSON.stringify({ token }),
    }),
  disconnectGitHub: () =>
    request<GitHubConnectionState>("/integrations/github", {
      method: "DELETE",
    }),
  listGitHubRepositories: (query = "") => {
    const params = new URLSearchParams();
    if (query) params.set("query", query);
    const suffix = params.toString();
    return request<GitHubRepositoryList>(
      `/integrations/github/repositories${suffix ? `?${suffix}` : ""}`,
    );
  },
  bindProjectGitHub: (
    projectId: string,
    input: { owner: string; repo: string; default_branch: string },
  ) =>
    request<ProjectGitHubBindingResult>(
      `/projects/${encodeURIComponent(projectId)}/github`,
      {
        method: "POST",
        body: JSON.stringify(input),
      },
    ),
  syncProjectIssues: (projectId: string) =>
    request<GitHubSyncState>(
      `/projects/${encodeURIComponent(projectId)}/issues/sync`,
      { method: "POST" },
    ),
  listProjectIssues: (projectId: string, filters: GitHubIssueFilters = {}) => {
    const params = new URLSearchParams();
    if (filters.state) params.set("state", filters.state);
    if (filters.label) params.set("label", filters.label);
    if (filters.query) params.set("query", filters.query);
    if (filters.limit !== undefined) params.set("limit", String(filters.limit));
    if (filters.offset !== undefined)
      params.set("offset", String(filters.offset));
    const suffix = params.toString();
    return request<GitHubIssueList>(
      `/projects/${encodeURIComponent(projectId)}/issues${suffix ? `?${suffix}` : ""}`,
    );
  },
  getProjectIssue: (projectId: string, number: number) =>
    request<GitHubIssueDetail>(
      `/projects/${encodeURIComponent(projectId)}/issues/${number}`,
    ),
  listParticipants: (filters: ParticipantFilters = {}) => {
    const params = new URLSearchParams();
    if (filters.role) params.set("role", filters.role);
    if (filters.provider) params.set("provider", filters.provider);
    if (filters.status) params.set("status", filters.status);
    if (filters.campaignId) params.set("campaign_id", filters.campaignId);
    if (filters.projectId) params.set("project_id", filters.projectId);
    if (filters.participantId)
      params.set("participant_id", filters.participantId);
    if (filters.limit !== undefined) params.set("limit", String(filters.limit));
    if (filters.offset !== undefined)
      params.set("offset", String(filters.offset));
    const query = params.toString();
    return request<ParticipantPage>(
      `/office/participants${query ? `?${query}` : ""}`,
    );
  },
  listToolEvents: (sessionRecordId: string, afterSequence = 0, limit = 100) =>
    request<ProviderToolEventProjection>(
      `/provider/sessions/${encodeURIComponent(sessionRecordId)}/tool_events?after_sequence=${afterSequence}&limit=${limit}`,
    ),
  getDecisions: () => request<DecisionList>("/office/decisions"),
  resolveModelCall: (id: string, actor: string, reason: string) =>
    request<void>(`/model/calls/${encodeURIComponent(id)}/resolve`, {
      method: "POST",
      body: JSON.stringify({ actor, reason }),
    }),
  getCandidateReceipts: (candidateId: string) =>
    request<QualityReceipt[]>(
      `/quality/candidates/${encodeURIComponent(candidateId)}/receipts`,
    ),
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
