import { useEffect, useMemo, useState } from "react";
import { Badge, type BadgeTone } from "../components/ui/Badge";
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
  type GitHubIssueSummary,
  type IssueIntakeBatchState,
  type IssueIntakePolicy,
  type IssueIntakeState,
  newIdempotencyKey,
  type ProjectConfig,
  type ProjectGitHubBinding,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";
import "./issue-intake.css";

const DAY_MS = 24 * 60 * 60 * 1000;

function message(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function localDeadline(value: string): string {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "";
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

function isoDeadline(value: string): string {
  if (!value) return "";
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toISOString() : "";
}

function splitList(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function splitNumbers(value: string): number[] {
  return splitList(value)
    .map((item) => Number.parseInt(item, 10))
    .filter((item) => Number.isInteger(item) && item > 0);
}

function batchTone(
  status: IssueIntakeBatchState["batch"]["status"],
): BadgeTone {
  switch (status) {
    case "ACTIVE":
      return "success";
    case "REVOKED":
      return "danger";
    case "EXPIRED":
      return "attention";
    default:
      return "neutral";
  }
}

function itemTone(state: string): BadgeTone {
  switch (state) {
    case "RECONCILED":
      return "success";
    case "PENDING":
      return "info";
    case "STALE":
      return "attention";
    case "INELIGIBLE":
    case "REVOKED":
      return "danger";
    default:
      return "neutral";
  }
}

function defaultExpiry(): string {
  return localDeadline(new Date(Date.now() + DAY_MS).toISOString());
}

function formFromPolicy(policy: IssueIntakePolicy | null) {
  return {
    requiredLabels: policy?.required_labels.join(", ") ?? "factory:r0",
    excludedLabels: policy?.excluded_labels.join(", ") ?? "",
    authorIds: policy?.author_ids.join(", ") ?? "",
    authorAssociations: policy?.author_associations ?? ["MEMBER"],
    allowedPaths: policy?.allowed_paths.join(", ") ?? "src",
    capabilities: policy?.capabilities.join(", ") ?? "repository-edit",
    maxIssues: String(policy?.max_issues ?? 3),
    maxCostMicrosPerIssue: String(policy?.max_cost_micros_per_issue ?? 300_000),
    maxTotalCostMicros: String(policy?.max_total_cost_micros ?? 1_000_000),
    maxAttempts: String(policy?.max_attempts ?? 2),
    maxSessions: String(policy?.max_sessions ?? 3),
    maxTurns: String(policy?.max_turns ?? 4),
    maxWallClockSecs: String(policy?.max_wall_clock_secs ?? 900),
    expiresAt: localDeadline(
      policy?.expires_at ?? new Date(Date.now() + 7 * DAY_MS).toISOString(),
    ),
    reason: "",
    acknowledge: false,
  };
}

export function IssueIntakePanel({
  projectId,
  binding,
  connectionConfigured,
  state,
  loading,
  loadError,
  selected,
  onRefresh,
  onSelectionClear,
}: {
  projectId: string;
  binding: ProjectGitHubBinding | null;
  connectionConfigured: boolean;
  state: IssueIntakeState | null;
  loading: boolean;
  loadError: string | null;
  selected: GitHubIssueSummary[];
  onRefresh: () => void;
  onSelectionClear: () => void;
}) {
  const [profiles, setProfiles] = useState<ExecutionProfile[]>([]);
  const [config, setConfig] = useState<ProjectConfig | null>(null);
  const [form, setForm] = useState(() => formFromPolicy(null));
  const [profileId, setProfileId] = useState("");
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmDisable, setConfirmDisable] = useState(false);
  const [disableReason, setDisableReason] = useState("");
  const [revokeReason, setRevokeReason] = useState("");
  const [batchExpiry, setBatchExpiry] = useState(defaultExpiry);
  const [batchBudget, setBatchBudget] = useState("");
  const [batchAcknowledge, setBatchAcknowledge] = useState(false);
  const [batchReason, setBatchReason] = useState("");

  const policy = state?.policy ?? null;
  const batches = state?.batches ?? [];
  const profile = profiles.find((item) => item.id === profileId) ?? null;

  useEffect(() => {
    let active = true;
    void Promise.all([
      coreApi.listExecutionProfiles(),
      coreApi.getProjectConfig(projectId),
    ])
      .then(([profileList, projectConfig]) => {
        if (!active) return;
        setProfiles(profileList.profiles);
        setConfig(projectConfig);
        setProfileId((current) => {
          if (current) return current;
          return profileList.profiles.find((item) => item.executable)?.id ?? "";
        });
      })
      .catch(() => {
        if (active) setProfiles([]);
      });
    return () => {
      active = false;
    };
    // Project selection is the reload key; policy edits do not change profiles.
  }, [projectId]);

  useEffect(() => {
    setForm(formFromPolicy(policy));
    setEditing(policy === null);
    if (policy) {
      setProfileId(policy.profile_id);
      setBatchBudget(
        String(
          Math.max(
            policy.max_cost_micros_per_issue,
            policy.max_total_cost_micros,
          ),
        ),
      );
    }
    setConfirmDisable(false);
  }, [policy]);

  useEffect(() => {
    if (config && !policy) {
      setForm((current) => ({
        ...current,
        allowedPaths:
          current.allowedPaths || config.allowedPaths.join(", ") || "src",
        capabilities:
          current.capabilities ||
          config.capabilities.join(", ") ||
          "repository-edit",
      }));
    }
  }, [config, policy]);

  const estimatedBatchCost = useMemo(() => {
    if (!policy) return 0;
    return selected.length * policy.max_cost_micros_per_issue;
  }, [policy, selected.length]);

  function update<K extends keyof typeof form>(
    key: K,
    value: (typeof form)[K],
  ) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  async function savePolicy(event: React.FormEvent) {
    event.preventDefault();
    setError(null);
    setNotice(null);
    if (!profile) {
      setError("Select an executable execution profile.");
      return;
    }
    if (!policy && !form.acknowledge) {
      setError("Acknowledge the exact policy before enabling issue intake.");
      return;
    }
    if (!binding?.repository_github_id) {
      setError(
        "The Project binding has no immutable repository ID. Rebind from Browse GitHub repositories.",
      );
      return;
    }
    if (!config) {
      setError("Project configuration is unavailable.");
      return;
    }
    if (form.reason.trim() === "") {
      setError("A reason is required for the audit record.");
      return;
    }
    setBusy("policy");
    try {
      await coreApi.configureIssueIntakePolicy(projectId, {
        repository_full_name: binding.repo_full_name,
        repository_github_id: binding.repository_github_id,
        required_labels: splitList(form.requiredLabels),
        excluded_labels: splitList(form.excludedLabels),
        author_ids: splitNumbers(form.authorIds),
        author_associations: form.authorAssociations,
        execution_profile_id: profile.id,
        profile_revision_id: profile.revisionId,
        profile_revision: profile.revision,
        profile_hash: profile.contentHash,
        project_config_hash: config.configHash,
        allowed_paths: splitList(form.allowedPaths),
        capabilities: splitList(form.capabilities),
        max_issues: Number.parseInt(form.maxIssues, 10) || 1,
        max_concurrent: 1,
        max_cost_micros_per_issue:
          Number.parseInt(form.maxCostMicrosPerIssue, 10) || 1,
        max_total_cost_micros:
          Number.parseInt(form.maxTotalCostMicros, 10) || 1,
        max_attempts: Number.parseInt(form.maxAttempts, 10) || 1,
        max_sessions: Number.parseInt(form.maxSessions, 10) || 2,
        max_turns: Number.parseInt(form.maxTurns, 10) || 1,
        max_wall_clock_secs: Number.parseInt(form.maxWallClockSecs, 10) || 120,
        expires_at: isoDeadline(form.expiresAt),
        acknowledge: true,
        reason: form.reason,
        idempotency_key: newIdempotencyKey(),
      });
      setNotice(
        "Issue-intake policy revision acknowledged. Execution remains disabled; every batch and proposal is still explicit.",
      );
      setEditing(false);
      onRefresh();
    } catch (cause) {
      setError(message(cause, "Issue-intake policy could not be saved."));
    } finally {
      setBusy(null);
    }
  }

  async function disablePolicy() {
    setBusy("disable");
    setError(null);
    setNotice(null);
    try {
      await coreApi.disableIssueIntakePolicy(projectId, {
        reason: disableReason.trim() || "operator disabled issue intake",
        idempotency_key: newIdempotencyKey(),
      });
      setNotice(
        "Issue intake is disabled. Active batches and their delegations were revoked.",
      );
      setConfirmDisable(false);
      setDisableReason("");
      onRefresh();
    } catch (cause) {
      setError(message(cause, "Issue intake could not be disabled."));
    } finally {
      setBusy(null);
    }
  }

  async function createBatch(event: React.FormEvent) {
    event.preventDefault();
    setError(null);
    setNotice(null);
    if (!policy) return;
    if (selected.length === 0) {
      setError("Select at least one exact issue snapshot.");
      return;
    }
    if (!batchAcknowledge) {
      setError(
        "Acknowledge the exact selected snapshots before creating a batch.",
      );
      return;
    }
    if (batchReason.trim() === "") {
      setError("A reason is required for the batch acknowledgement.");
      return;
    }
    const budget = Number.parseInt(batchBudget, 10);
    if (!Number.isInteger(budget) || budget < estimatedBatchCost) {
      setError(
        `The batch budget must cover the selected snapshots (minimum ${estimatedBatchCost} micros).`,
      );
      return;
    }
    setBusy("batch");
    try {
      await coreApi.createIssueIntakeBatch(projectId, {
        policy_id: policy.id,
        expected_policy_revision: policy.revision,
        expected_policy_hash: policy.content_hash,
        items: selected.map((issue) => ({
          number: issue.number,
          github_issue_id: issue.github_id,
          content_hash: issue.content_hash,
          updated_at: issue.updated_at,
        })),
        max_total_cost_micros: budget,
        expires_at: isoDeadline(batchExpiry),
        acknowledge: true,
        reason: batchReason,
        idempotency_key: newIdempotencyKey(),
      });
      setNotice(
        `Acknowledged ${selected.length} exact snapshot(s). Reconciliation still requires a separate operator action and never starts a Campaign.`,
      );
      setBatchAcknowledge(false);
      setBatchReason("");
      onSelectionClear();
      onRefresh();
    } catch (cause) {
      const code = cause instanceof CoreApiError ? cause.reason : null;
      setError(
        `${message(cause, "Batch acknowledgement failed.")}${code ? ` (${code})` : ""}`,
      );
      onRefresh();
    } finally {
      setBusy(null);
    }
  }

  async function dispatchQueuedItem(batchId: string) {
    setBusy(`dispatch-${batchId}`);
    setError(null);
    setNotice(null);
    try {
      const result = await coreApi.dispatchIssueIntake(projectId, batchId);
      if (result.status === "RECONCILED") {
        setNotice(
          result.intake
            ? `Proposal ${shortId(result.intake.id)} created as ${result.intake.mode}. No Campaign was started and no provider call was made.`
            : "Queue item replayed from its committed proposal. No Campaign was started.",
        );
      } else {
        setError(
          `Reconciliation ${result.status}${result.reasons.length > 0 ? `: ${result.reasons.join(", ")}` : ""}`,
        );
      }
      onRefresh();
    } catch (cause) {
      const code = cause instanceof CoreApiError ? cause.reason : null;
      setError(
        `${message(cause, "Issue reconciliation failed before creating a proposal.")}${code ? ` (${code})` : ""}`,
      );
      onRefresh();
    } finally {
      setBusy(null);
    }
  }

  async function revokeBatch(batch: IssueIntakeBatchState["batch"]) {
    setBusy(`revoke-${batch.id}`);
    setError(null);
    setNotice(null);
    try {
      await coreApi.revokeIssueIntakeBatch(projectId, batch.id, {
        reason: revokeReason.trim() || "operator revoked issue batch",
      });
      setNotice("Batch revoked. Existing reservations are retained.");
      setRevokeReason("");
      onRefresh();
    } catch (cause) {
      setError(message(cause, "Batch could not be revoked."));
    } finally {
      setBusy(null);
    }
  }

  if (loading) {
    return (
      <Panel className="issues-panel">
        <LoadingState label="Loading issue-intake policy…" />
      </Panel>
    );
  }

  if (loadError) {
    return (
      <Panel className="issues-panel">
        <ErrorState
          title="Issue intake unavailable"
          message={loadError}
          action={<Button onClick={onRefresh}>Retry</Button>}
        />
      </Panel>
    );
  }

  return (
    <Panel className="issues-panel issue-intake-panel">
      <div className="issues-panel-heading">
        <h2>Governed issue intake</h2>
        {policy?.enabled ? (
          <Badge tone="success">ENABLED</Badge>
        ) : (
          <Badge tone="neutral">OFF</Badge>
        )}
      </div>
      <p className="issues-note">
        Default off. A policy acknowledges exact repository, author, label,
        profile, path and budget bounds. A batch acknowledges exact issue
        snapshots. Neither starts a Campaign: execution, approval and Worker
        dispatch remain separate explicit operations.
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      {policy && !editing && (
        <>
          <dl className="issues-facts issue-intake-facts">
            <div>
              <dt>Policy revision</dt>
              <dd>{policy.revision}</dd>
            </div>
            <div>
              <dt>Repository</dt>
              <dd className="mono">
                {policy.repository_full_name} (ID {policy.repository_github_id})
              </dd>
            </div>
            <div>
              <dt>Required labels</dt>
              <dd>{policy.required_labels.join(", ") || "—"}</dd>
            </div>
            <div>
              <dt>Excluded labels</dt>
              <dd>{policy.excluded_labels.join(", ") || "—"}</dd>
            </div>
            <div>
              <dt>Author IDs</dt>
              <dd>{policy.author_ids.join(", ")}</dd>
            </div>
            <div>
              <dt>Author association</dt>
              <dd>{policy.author_associations.join(", ")}</dd>
            </div>
            <div>
              <dt>Profile</dt>
              <dd className="mono">
                {shortId(policy.profile_id)} rev {policy.profile_revision}
              </dd>
            </div>
            <div>
              <dt>Risk ceiling</dt>
              <dd>{policy.max_risk_class}</dd>
            </div>
            <div>
              <dt>Paths</dt>
              <dd>{policy.allowed_paths.join(", ")}</dd>
            </div>
            <div>
              <dt>Capabilities</dt>
              <dd>{policy.capabilities.join(", ")}</dd>
            </div>
            <div>
              <dt>Max issues / concurrency</dt>
              <dd>
                {policy.max_issues} / {policy.max_concurrent}
              </dd>
            </div>
            <div>
              <dt>Cost per issue / batch</dt>
              <dd>
                {policy.max_cost_micros_per_issue} /{" "}
                {policy.max_total_cost_micros} micros
              </dd>
            </div>
            <div>
              <dt>Expires</dt>
              <dd>{formatTimestamp(policy.expires_at)}</dd>
            </div>
            <div>
              <dt>Auto-start</dt>
              <dd>not granted by this policy</dd>
            </div>
          </dl>
          <p className="issues-note">
            Profile autoStart is ignored for issue intake. This policy only
            permits deterministic reconciliation into ordinary proposals.
          </p>
          <div className="issues-actions">
            <Button onClick={() => setEditing(true)} disabled={busy !== null}>
              Edit and re-acknowledge
            </Button>
            {confirmDisable ? (
              <div className="issue-intake-inline">
                <Input
                  label="Disable reason"
                  value={disableReason}
                  onChange={(event) => setDisableReason(event.target.value)}
                  placeholder="why issue intake is being disabled"
                />
                <Button
                  variant="danger"
                  onClick={() => void disablePolicy()}
                  disabled={busy !== null}
                >
                  {busy === "disable" ? "Disabling…" : "Confirm disable"}
                </Button>
                <Button
                  variant="quiet"
                  onClick={() => setConfirmDisable(false)}
                  disabled={busy !== null}
                >
                  Cancel
                </Button>
              </div>
            ) : (
              <Button
                variant="danger"
                onClick={() => setConfirmDisable(true)}
                disabled={busy !== null}
              >
                Disable issue intake
              </Button>
            )}
          </div>
        </>
      )}

      {(editing || !policy) && (
        <form className="issue-intake-form" onSubmit={savePolicy}>
          {!binding && (
            <p className="error" role="alert">
              Bind a GitHub repository before enabling issue intake.
            </p>
          )}
          {binding && !binding.repository_github_id && (
            <p className="error" role="alert">
              This binding predates immutable repository identity. Rebind from
              Browse GitHub repositories. Labels alone never authorize intake.
            </p>
          )}
          {!connectionConfigured && (
            <p className="error" role="alert">
              GitHub is not connected. Connect a credential before enabling
              issue intake.
            </p>
          )}
          <div className="issue-intake-grid">
            <Input
              label="Bound repository"
              value={binding?.repo_full_name ?? ""}
              readOnly
              help="Bound repository; the immutable GitHub repository ID is required."
            />
            <Select
              label="Execution profile"
              value={profileId}
              onChange={(event) => setProfileId(event.target.value)}
              help="Only executable exact revisions can be acknowledged."
            >
              <option value="">Select a profile…</option>
              {profiles.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name} rev {item.revision}
                  {item.executable ? "" : " (not executable)"}
                </option>
              ))}
            </Select>
            {profile?.autoStart.enabled && (
              <p className="notice" role="status">
                This profile has autoStart enabled. Issue intake ignores it: a
                policy, batch and explicit approval are still required, and no
                autonomous Campaign start is available from this panel.
              </p>
            )}
            <Input
              label="Required labels (comma separated)"
              value={form.requiredLabels}
              onChange={(event) => update("requiredLabels", event.target.value)}
              help="All listed labels must be present. Labels alone never authorize anything."
            />
            <Input
              label="Excluded labels"
              value={form.excludedLabels}
              onChange={(event) => update("excludedLabels", event.target.value)}
            />
            <Input
              label="Author IDs (immutable GitHub user IDs)"
              value={form.authorIds}
              onChange={(event) => update("authorIds", event.target.value)}
              placeholder="1234, 5678"
              help="Logins are not identity. Unknown author IDs fail closed."
            />
            <fieldset className="issue-intake-associations">
              <legend>Allowed associations</legend>
              {["OWNER", "MEMBER", "COLLABORATOR"].map((association) => (
                <Checkbox
                  key={association}
                  label={association}
                  checked={form.authorAssociations.includes(association)}
                  onChange={(event) =>
                    update(
                      "authorAssociations",
                      event.target.checked
                        ? [...form.authorAssociations, association]
                        : form.authorAssociations.filter(
                            (item) => item !== association,
                          ),
                    )
                  }
                />
              ))}
            </fieldset>
            <Input
              label="Allowed paths (comma separated)"
              value={form.allowedPaths}
              onChange={(event) => update("allowedPaths", event.target.value)}
              help="Must be inside the Project configuration. Protected policy, delivery, credential and migration paths are rejected."
            />
            <Input
              label="Capabilities"
              value={form.capabilities}
              onChange={(event) => update("capabilities", event.target.value)}
              help="Kebab-case capabilities already present in the Project configuration."
            />
            <Input
              label="Max issues per batch"
              type="number"
              min={1}
              max={20}
              value={form.maxIssues}
              onChange={(event) => update("maxIssues", event.target.value)}
            />
            <Input
              label="Max cost per issue (micros)"
              type="number"
              min={1}
              value={form.maxCostMicrosPerIssue}
              onChange={(event) =>
                update("maxCostMicrosPerIssue", event.target.value)
              }
            />
            <Input
              label="Max batch cost (micros)"
              type="number"
              min={1}
              value={form.maxTotalCostMicros}
              onChange={(event) =>
                update("maxTotalCostMicros", event.target.value)
              }
            />
            <Input
              label="Max attempts per request"
              type="number"
              min={1}
              max={5}
              value={form.maxAttempts}
              onChange={(event) => update("maxAttempts", event.target.value)}
            />
            <Input
              label="Max sessions per request"
              type="number"
              min={1}
              value={form.maxSessions}
              onChange={(event) => update("maxSessions", event.target.value)}
            />
            <Input
              label="Max turns per request"
              type="number"
              min={1}
              value={form.maxTurns}
              onChange={(event) => update("maxTurns", event.target.value)}
            />
            <Input
              label="Max wall clock (seconds)"
              type="number"
              min={120}
              value={form.maxWallClockSecs}
              onChange={(event) =>
                update("maxWallClockSecs", event.target.value)
              }
            />
            <Input
              label="Policy expiry"
              type="datetime-local"
              value={form.expiresAt}
              onChange={(event) => update("expiresAt", event.target.value)}
            />
          </div>
          <Textarea
            label="Acknowledgement reason"
            value={form.reason}
            onChange={(event) => update("reason", event.target.value)}
            rows={2}
            required
            help="Stored in the append-only audit. Do not include secrets or issue bodies."
          />
          <Checkbox
            label="I acknowledge this exact repository, author, label, profile, path, R0 and budget policy. This does not start any Campaign."
            checked={form.acknowledge}
            onChange={(event) => update("acknowledge", event.target.checked)}
          />
          <div className="issues-actions">
            <Button
              type="submit"
              variant="primary"
              disabled={
                busy !== null ||
                !binding?.repository_github_id ||
                !connectionConfigured ||
                !profile?.executable ||
                !form.acknowledge ||
                form.reason.trim() === ""
              }
            >
              {busy === "policy"
                ? "Acknowledging…"
                : policy
                  ? "Save new policy revision"
                  : "Enable issue intake"}
            </Button>
            {policy && (
              <Button
                variant="quiet"
                onClick={() => setEditing(false)}
                disabled={busy !== null}
              >
                Cancel
              </Button>
            )}
          </div>
        </form>
      )}

      {policy?.enabled && (
        <>
          <div className="issue-intake-selection">
            <div>
              <h3>Exact snapshot selection</h3>
              <p className="muted">
                {selected.length === 0
                  ? "Select issues with the checkboxes in the synchronized issue table. Selection is manual only; there is no subscription to future issues."
                  : `${selected.length} snapshot(s) selected. Maximum acknowledged cost ${estimatedBatchCost} micros.`}
              </p>
            </div>
            <Badge tone={selected.length > 0 ? "info" : "neutral"}>
              {selected.length} SELECTED
            </Badge>
          </div>
          <form className="issue-intake-batch-form" onSubmit={createBatch}>
            <div className="issue-intake-grid">
              <Input
                label="Batch budget (micros)"
                type="number"
                min={estimatedBatchCost || 1}
                value={batchBudget}
                onChange={(event) => setBatchBudget(event.target.value)}
                help={`Must cover the ${selected.length} selected snapshot(s): at least ${estimatedBatchCost} micros.`}
              />
              <Input
                label="Batch expiry"
                type="datetime-local"
                value={batchExpiry}
                onChange={(event) => setBatchExpiry(event.target.value)}
              />
            </div>
            <Textarea
              label="Batch reason"
              value={batchReason}
              onChange={(event) => setBatchReason(event.target.value)}
              rows={2}
              help="No issue body is copied into audit or telemetry."
            />
            <Checkbox
              label="I acknowledge these exact issue IDs and content digests. A later edit or new issue requires a new acknowledgement."
              checked={batchAcknowledge}
              onChange={(event) => setBatchAcknowledge(event.target.checked)}
            />
            <Button
              type="submit"
              variant="primary"
              disabled={
                busy !== null ||
                selected.length === 0 ||
                !batchAcknowledge ||
                batchReason.trim() === "" ||
                !policy.enabled
              }
            >
              {busy === "batch"
                ? "Acknowledging batch…"
                : "Acknowledge finite batch"}
            </Button>
          </form>

          <div className="issue-intake-batches">
            <h3>Batches and durable queue</h3>
            {batches.length === 0 ? (
              <EmptyState
                title="No active batch"
                description="A batch is the finite acknowledgement of exact issue snapshots. It does not authorize a Campaign."
              />
            ) : (
              <>
                <Input
                  label="Revocation reason"
                  value={revokeReason}
                  onChange={(event) => setRevokeReason(event.target.value)}
                  placeholder="optional; defaults to operator revoked issue batch"
                />
                {state?.batches.map((batchState) => (
                  <div className="issue-intake-batch" key={batchState.batch.id}>
                    <div className="issue-intake-batch-heading">
                      <div>
                        <strong className="mono">
                          {shortId(batchState.batch.id)}
                        </strong>
                        <span className="muted">
                          {" "}
                          policy rev {batchState.batch.policy_revision} ·{" "}
                          {batchState.batch.max_issues} issue(s) · expires{" "}
                          {formatTimestamp(batchState.batch.expires_at)}
                        </span>
                      </div>
                      <Badge tone={batchTone(batchState.batch.status)}>
                        {batchState.batch.status}
                      </Badge>
                    </div>
                    <p className="muted">
                      Remaining issues {batchState.remaining_issues}; remaining
                      cost {batchState.remaining_cost_micros} micros.
                      Acknowledgement does not grant execution or merge
                      authority.
                    </p>
                    <ul className="issue-intake-items">
                      {batchState.items.map(({ item, current_eligibility }) => (
                        <li key={item.id}>
                          <div className="issue-intake-item-heading">
                            <span className="mono">#{item.issue_number}</span>
                            <Badge tone={itemTone(item.state)}>
                              {item.state}
                            </Badge>
                            {current_eligibility &&
                              item.state === "PENDING" && (
                                <Badge
                                  tone={
                                    current_eligibility.eligible
                                      ? "success"
                                      : "danger"
                                  }
                                >
                                  {current_eligibility.eligible
                                    ? "ELIGIBLE"
                                    : "BLOCKED"}
                                </Badge>
                              )}
                          </div>
                          <span className="muted">
                            author {item.author_login ?? "unknown"} (ID{" "}
                            {item.author_id ?? "unknown"},{" "}
                            {item.author_association ?? "unknown"}) · digest{" "}
                            <span className="mono">
                              {item.content_hash.slice(0, 12)}…
                            </span>
                          </span>
                          {current_eligibility &&
                            item.state === "PENDING" &&
                            current_eligibility.reasons.length > 0 && (
                              <ul className="issue-intake-reasons">
                                {current_eligibility.reasons.map((reason) => (
                                  <li key={reason}>{reason}</li>
                                ))}
                              </ul>
                            )}
                          {item.state !== "PENDING" &&
                            item.eligibility_reasons.length > 0 && (
                              <ul className="issue-intake-reasons">
                                {item.eligibility_reasons.map((reason) => (
                                  <li key={reason}>{reason}</li>
                                ))}
                              </ul>
                            )}
                          {item.intake_request_id && (
                            <span className="muted">
                              Proposal intake{" "}
                              <span className="mono">
                                {shortId(item.intake_request_id)}
                              </span>{" "}
                              recorded. Campaign start remains separate.
                            </span>
                          )}
                        </li>
                      ))}
                    </ul>
                    {batchState.batch.status === "ACTIVE" && (
                      <Button
                        variant="danger"
                        size="sm"
                        onClick={() => void revokeBatch(batchState.batch)}
                        disabled={busy !== null}
                      >
                        {busy === `revoke-${batchState.batch.id}`
                          ? "Revoking…"
                          : "Revoke batch"}
                      </Button>
                    )}
                  </div>
                ))}
              </>
            )}
          </div>
        </>
      )}

      {state && state.queue.length > 0 && (
        <div className="issue-intake-batches">
          <h3>Durable reconciliation queue</h3>
          <p className="muted">
            A dispatch performs one direct GitHub freshness read and may create
            one ordinary REVIEW_PLAN proposal. Execution stays disabled; a
            Campaign still requires separate explicit approval.
          </p>
          <ul className="issue-intake-items">
            {state.queue.map((queued) => {
              const batchState = state.batches.find(
                (candidate) => candidate.batch.id === queued.batch_id,
              );
              const canDispatch =
                queued.status === "PENDING" &&
                batchState?.batch.status === "ACTIVE";
              return (
                <li key={queued.id}>
                  <div className="issue-intake-item-heading">
                    <span className="mono">#{queued.issue_number}</span>
                    <Badge tone={itemTone(queued.status)}>
                      {queued.status}
                    </Badge>
                    <span className="muted">
                      attempt {queued.attempts}/{queued.max_attempts}
                      {queued.next_attempt_at
                        ? ` · next ${formatTimestamp(queued.next_attempt_at)}`
                        : ""}
                    </span>
                  </div>
                  {queued.reason && (
                    <span className="muted">{queued.reason}</span>
                  )}
                  {queued.reasons.length > 0 && (
                    <ul className="issue-intake-reasons">
                      {queued.reasons.map((reason) => (
                        <li key={reason}>{reason}</li>
                      ))}
                    </ul>
                  )}
                  {queued.status === "RECONCILED" &&
                    queued.intake_request_id && (
                      <span className="muted">
                        Proposal{" "}
                        <span className="mono">
                          {shortId(queued.intake_request_id)}
                        </span>{" "}
                        · Lead{" "}
                        <span className="mono">
                          {shortId(queued.lead_request_id ?? "")}
                        </span>{" "}
                        · no Campaign started.
                      </span>
                    )}
                  {canDispatch && (
                    <div className="issues-actions">
                      <Button
                        size="sm"
                        onClick={() => void dispatchQueuedItem(queued.batch_id)}
                        disabled={busy !== null}
                      >
                        {busy === `dispatch-${queued.batch_id}`
                          ? "Checking freshness…"
                          : "Create proposal"}
                      </Button>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </div>
      )}

      {state && state.audit.length > 0 && (
        <details className="issues-details">
          <summary>Issue-intake audit ({state.audit.length})</summary>
          <ul className="issues-audit">
            {state.audit.map((entry) => (
              <li key={entry.audit_id}>
                <strong>{entry.action}</strong> by {entry.actor} —{" "}
                {formatTimestamp(entry.created_at)}
                {entry.reason ? ` — ${entry.reason}` : ""}
                {entry.detail ? ` — ${entry.detail}` : ""}
              </li>
            ))}
          </ul>
        </details>
      )}
    </Panel>
  );
}
