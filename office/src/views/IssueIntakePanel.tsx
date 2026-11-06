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
import { shortId } from "../format";
import { useI18n } from "../i18n";
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
  const { t, formatDate } = useI18n();
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
      setError(t("issues.intake.error.profile"));
      return;
    }
    if (!policy && !form.acknowledge) {
      setError(t("issues.intake.error.acknowledgePolicy"));
      return;
    }
    if (!binding?.repository_github_id) {
      setError(t("issues.intake.error.repositoryIdentity"));
      return;
    }
    if (!config) {
      setError(t("issues.intake.error.configUnavailable"));
      return;
    }
    if (form.reason.trim() === "") {
      setError(t("issues.intake.error.reason"));
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
      setNotice(t("issues.intake.notice.policySaved"));
      setEditing(false);
      onRefresh();
    } catch (cause) {
      setError(message(cause, t("issues.intake.error.savePolicy")));
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
        reason: disableReason.trim() || t("issues.intake.defaultDisableReason"),
        idempotency_key: newIdempotencyKey(),
      });
      setNotice(t("issues.intake.notice.disabled"));
      setConfirmDisable(false);
      setDisableReason("");
      onRefresh();
    } catch (cause) {
      setError(message(cause, t("issues.intake.error.disable")));
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
      setError(t("issues.intake.error.noSelection"));
      return;
    }
    if (!batchAcknowledge) {
      setError(t("issues.intake.error.acknowledgeBatch"));
      return;
    }
    if (batchReason.trim() === "") {
      setError(t("issues.intake.error.batchReason"));
      return;
    }
    const budget = Number.parseInt(batchBudget, 10);
    if (!Number.isInteger(budget) || budget < estimatedBatchCost) {
      setError(
        t("issues.intake.error.batchBudget", { minimum: estimatedBatchCost }),
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
        t("issues.intake.notice.batchCreated", { count: selected.length }),
      );
      setBatchAcknowledge(false);
      setBatchReason("");
      onSelectionClear();
      onRefresh();
    } catch (cause) {
      const code = cause instanceof CoreApiError ? cause.reason : null;
      const detail = message(cause, t("issues.intake.error.batch"));
      setError(
        code
          ? t("issues.intake.error.withCode", { message: detail, code })
          : detail,
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
            ? t("issues.intake.notice.proposalCreated", {
                id: shortId(result.intake.id),
                mode: result.intake.mode,
              })
            : t("issues.intake.notice.proposalReplayed"),
        );
      } else {
        setError(
          result.reasons.length > 0
            ? t("issues.intake.error.reconciliationReasons", {
                status: result.status,
                reasons: result.reasons.join(", "),
              })
            : t("issues.intake.error.reconciliation", {
                status: result.status,
              }),
        );
      }
      onRefresh();
    } catch (cause) {
      const code = cause instanceof CoreApiError ? cause.reason : null;
      const detail = message(cause, t("issues.intake.error.dispatch"));
      setError(
        code
          ? t("issues.intake.error.withCode", { message: detail, code })
          : detail,
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
        reason: revokeReason.trim() || t("issues.intake.defaultRevokeReason"),
      });
      setNotice(t("issues.intake.notice.batchRevoked"));
      setRevokeReason("");
      onRefresh();
    } catch (cause) {
      setError(message(cause, t("issues.intake.error.revoke")));
    } finally {
      setBusy(null);
    }
  }

  if (loading) {
    return (
      <Panel className="issues-panel">
        <LoadingState label={t("issues.intake.loading")} />
      </Panel>
    );
  }

  if (loadError) {
    return (
      <Panel className="issues-panel">
        <ErrorState
          title={t("issues.intake.unavailableTitle")}
          message={loadError}
          action={<Button onClick={onRefresh}>{t("common.retry")}</Button>}
        />
      </Panel>
    );
  }

  return (
    <Panel className="issues-panel issue-intake-panel">
      <div className="issues-panel-heading">
        <h2>{t("issues.intake.title")}</h2>
        {policy?.enabled ? (
          <Badge tone="success">{t("issues.intake.badge.enabled")}</Badge>
        ) : (
          <Badge tone="neutral">{t("issues.intake.badge.off")}</Badge>
        )}
      </div>
      <p className="issues-note">{t("issues.intake.description")}</p>
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
              <dt>{t("issues.intake.fact.policyRevision")}</dt>
              <dd>{policy.revision}</dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.repository")}</dt>
              <dd className="mono">
                {t("issues.intake.fact.repositoryValue", {
                  name: policy.repository_full_name,
                  id: policy.repository_github_id ?? "",
                })}
              </dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.requiredLabels")}</dt>
              <dd>{policy.required_labels.join(", ") || "—"}</dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.excludedLabels")}</dt>
              <dd>{policy.excluded_labels.join(", ") || "—"}</dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.authorIds")}</dt>
              <dd>{policy.author_ids.join(", ")}</dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.authorAssociation")}</dt>
              <dd>{policy.author_associations.join(", ")}</dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.profile")}</dt>
              <dd className="mono">
                {t("issues.intake.fact.profileValue", {
                  id: shortId(policy.profile_id),
                  revision: policy.profile_revision,
                })}
              </dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.riskCeiling")}</dt>
              <dd>{policy.max_risk_class}</dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.paths")}</dt>
              <dd>{policy.allowed_paths.join(", ")}</dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.capabilities")}</dt>
              <dd>{policy.capabilities.join(", ")}</dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.maxIssues")}</dt>
              <dd>
                {policy.max_issues} / {policy.max_concurrent}
              </dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.cost")}</dt>
              <dd>
                {t("issues.intake.fact.costValue", {
                  perIssue: policy.max_cost_micros_per_issue,
                  perBatch: policy.max_total_cost_micros,
                })}
              </dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.expires")}</dt>
              <dd>{formatDate(policy.expires_at)}</dd>
            </div>
            <div>
              <dt>{t("issues.intake.fact.autoStart")}</dt>
              <dd>{t("issues.intake.fact.autoStartValue")}</dd>
            </div>
          </dl>
          <p className="issues-note">{t("issues.intake.fact.autoStartNote")}</p>
          <div className="issues-actions">
            <Button onClick={() => setEditing(true)} disabled={busy !== null}>
              {t("issues.intake.editPolicy")}
            </Button>
            {confirmDisable ? (
              <div className="issue-intake-inline">
                <Input
                  label={t("issues.intake.disableReason")}
                  value={disableReason}
                  onChange={(event) => setDisableReason(event.target.value)}
                  placeholder={t("issues.intake.disableReasonPlaceholder")}
                />
                <Button
                  variant="danger"
                  onClick={() => void disablePolicy()}
                  disabled={busy !== null}
                >
                  {busy === "disable"
                    ? t("issues.intake.disabling")
                    : t("issues.intake.confirmDisable")}
                </Button>
                <Button
                  variant="quiet"
                  onClick={() => setConfirmDisable(false)}
                  disabled={busy !== null}
                >
                  {t("issues.intake.cancel")}
                </Button>
              </div>
            ) : (
              <Button
                variant="danger"
                onClick={() => setConfirmDisable(true)}
                disabled={busy !== null}
              >
                {t("issues.intake.disable")}
              </Button>
            )}
          </div>
        </>
      )}

      {(editing || !policy) && (
        <form className="issue-intake-form" onSubmit={savePolicy}>
          {!binding && (
            <p className="error" role="alert">
              {t("issues.intake.error.noBinding")}
            </p>
          )}
          {binding && !binding.repository_github_id && (
            <p className="error" role="alert">
              {t("issues.intake.error.legacyBinding")}
            </p>
          )}
          {!connectionConfigured && (
            <p className="error" role="alert">
              {t("issues.intake.error.noConnection")}
            </p>
          )}
          <div className="issue-intake-grid">
            <Input
              label={t("issues.intake.field.repository")}
              value={binding?.repo_full_name ?? ""}
              readOnly
              help={t("issues.intake.field.repositoryHelp")}
            />
            <Select
              label={t("issues.intake.field.profile")}
              value={profileId}
              onChange={(event) => setProfileId(event.target.value)}
              help={t("issues.intake.field.profileHelp")}
            >
              <option value="">{t("issues.intake.field.profileSelect")}</option>
              {profiles.map((item) => (
                <option key={item.id} value={item.id}>
                  {t("issues.intake.field.profileOption", {
                    name: item.name,
                    revision: item.revision,
                  })}
                  {item.executable
                    ? ""
                    : t("issues.intake.field.notExecutable")}
                </option>
              ))}
            </Select>
            {profile?.autoStart.enabled && (
              <p className="notice" role="status">
                {t("issues.intake.notice.autoStart")}
              </p>
            )}
            <Input
              label={t("issues.intake.field.requiredLabels")}
              value={form.requiredLabels}
              onChange={(event) => update("requiredLabels", event.target.value)}
              help={t("issues.intake.field.requiredLabelsHelp")}
            />
            <Input
              label={t("issues.intake.field.excludedLabels")}
              value={form.excludedLabels}
              onChange={(event) => update("excludedLabels", event.target.value)}
            />
            <Input
              label={t("issues.intake.field.authorIds")}
              value={form.authorIds}
              onChange={(event) => update("authorIds", event.target.value)}
              placeholder="1234, 5678"
              help={t("issues.intake.field.authorIdsHelp")}
            />
            <fieldset className="issue-intake-associations">
              <legend>{t("issues.intake.field.associations")}</legend>
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
              label={t("issues.intake.field.paths")}
              value={form.allowedPaths}
              onChange={(event) => update("allowedPaths", event.target.value)}
              help={t("issues.intake.field.pathsHelp")}
            />
            <Input
              label={t("issues.intake.field.capabilities")}
              value={form.capabilities}
              onChange={(event) => update("capabilities", event.target.value)}
              help={t("issues.intake.field.capabilitiesHelp")}
            />
            <Input
              label={t("issues.intake.field.maxIssues")}
              type="number"
              min={1}
              max={20}
              value={form.maxIssues}
              onChange={(event) => update("maxIssues", event.target.value)}
            />
            <Input
              label={t("issues.intake.field.maxCostPerIssue")}
              type="number"
              min={1}
              value={form.maxCostMicrosPerIssue}
              onChange={(event) =>
                update("maxCostMicrosPerIssue", event.target.value)
              }
            />
            <Input
              label={t("issues.intake.field.maxBatchCost")}
              type="number"
              min={1}
              value={form.maxTotalCostMicros}
              onChange={(event) =>
                update("maxTotalCostMicros", event.target.value)
              }
            />
            <Input
              label={t("issues.intake.field.maxAttempts")}
              type="number"
              min={1}
              max={5}
              value={form.maxAttempts}
              onChange={(event) => update("maxAttempts", event.target.value)}
            />
            <Input
              label={t("issues.intake.field.maxSessions")}
              type="number"
              min={1}
              value={form.maxSessions}
              onChange={(event) => update("maxSessions", event.target.value)}
            />
            <Input
              label={t("issues.intake.field.maxTurns")}
              type="number"
              min={1}
              value={form.maxTurns}
              onChange={(event) => update("maxTurns", event.target.value)}
            />
            <Input
              label={t("issues.intake.field.maxWallClock")}
              type="number"
              min={120}
              value={form.maxWallClockSecs}
              onChange={(event) =>
                update("maxWallClockSecs", event.target.value)
              }
            />
            <Input
              label={t("issues.intake.field.policyExpiry")}
              type="datetime-local"
              value={form.expiresAt}
              onChange={(event) => update("expiresAt", event.target.value)}
            />
          </div>
          <Textarea
            label={t("issues.intake.field.reason")}
            value={form.reason}
            onChange={(event) => update("reason", event.target.value)}
            rows={2}
            required
            help={t("issues.intake.field.reasonHelp")}
          />
          <Checkbox
            label={t("issues.intake.field.acknowledge")}
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
                ? t("issues.intake.acknowledging")
                : policy
                  ? t("issues.intake.saveRevision")
                  : t("issues.intake.enable")}
            </Button>
            {policy && (
              <Button
                variant="quiet"
                onClick={() => setEditing(false)}
                disabled={busy !== null}
              >
                {t("issues.intake.cancel")}
              </Button>
            )}
          </div>
        </form>
      )}

      {policy?.enabled && (
        <>
          <div className="issue-intake-selection">
            <div>
              <h3>{t("issues.intake.selection.title")}</h3>
              <p className="muted">
                {selected.length === 0
                  ? t("issues.intake.selection.empty")
                  : t("issues.intake.selection.selected", {
                      count: selected.length,
                      cost: estimatedBatchCost,
                    })}
              </p>
            </div>
            <Badge tone={selected.length > 0 ? "info" : "neutral"}>
              {t("issues.intake.selection.badge", { count: selected.length })}
            </Badge>
          </div>
          <form className="issue-intake-batch-form" onSubmit={createBatch}>
            <div className="issue-intake-grid">
              <Input
                label={t("issues.intake.field.batchBudget")}
                type="number"
                min={estimatedBatchCost || 1}
                value={batchBudget}
                onChange={(event) => setBatchBudget(event.target.value)}
                help={t("issues.intake.field.batchBudgetHelp", {
                  count: selected.length,
                  minimum: estimatedBatchCost,
                })}
              />
              <Input
                label={t("issues.intake.field.batchExpiry")}
                type="datetime-local"
                value={batchExpiry}
                onChange={(event) => setBatchExpiry(event.target.value)}
              />
            </div>
            <Textarea
              label={t("issues.intake.field.batchReason")}
              value={batchReason}
              onChange={(event) => setBatchReason(event.target.value)}
              rows={2}
              help={t("issues.intake.field.batchReasonHelp")}
            />
            <Checkbox
              label={t("issues.intake.field.batchAcknowledge")}
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
                ? t("issues.intake.acknowledgingBatch")
                : t("issues.intake.acknowledgeBatch")}
            </Button>
          </form>

          <div className="issue-intake-batches">
            <h3>{t("issues.intake.batches.title")}</h3>
            {batches.length === 0 ? (
              <EmptyState
                title={t("issues.intake.batches.emptyTitle")}
                description={t("issues.intake.batches.emptyDescription")}
              />
            ) : (
              <>
                <Input
                  label={t("issues.intake.field.revokeReason")}
                  value={revokeReason}
                  onChange={(event) => setRevokeReason(event.target.value)}
                  placeholder={t("issues.intake.field.revokeReasonPlaceholder")}
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
                          {t("issues.intake.batch.meta", {
                            revision: batchState.batch.policy_revision,
                            count: batchState.batch.max_issues,
                            date: formatDate(batchState.batch.expires_at),
                          })}
                        </span>
                      </div>
                      <Badge tone={batchTone(batchState.batch.status)}>
                        {batchState.batch.status}
                      </Badge>
                    </div>
                    <p className="muted">
                      {t("issues.intake.batch.remaining", {
                        issues: batchState.remaining_issues,
                        cost: batchState.remaining_cost_micros,
                      })}
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
                                    ? t("issues.intake.eligibility.eligible")
                                    : t("issues.intake.eligibility.blocked")}
                                </Badge>
                              )}
                          </div>
                          <span className="muted">
                            {t("issues.intake.item.meta", {
                              author: item.author_login ?? t("common.unknown"),
                              id: item.author_id ?? t("common.unknown"),
                              association:
                                item.author_association ?? t("common.unknown"),
                            })}{" "}
                            · {t("issues.intake.item.digest")}{" "}
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
                              {t("issues.intake.batch.proposalIntake")}{" "}
                              <span className="mono">
                                {shortId(item.intake_request_id)}
                              </span>{" "}
                              {t("issues.intake.batch.proposalRecorded")}
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
                          ? t("issues.intake.revokingBatch")
                          : t("issues.intake.revokeBatch")}
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
          <h3>{t("issues.intake.queue.title")}</h3>
          <p className="muted">{t("issues.intake.queue.description")}</p>
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
                      {t("issues.intake.queue.attempt", {
                        attempts: queued.attempts,
                        max: queued.max_attempts,
                      })}
                      {queued.next_attempt_at
                        ? t("issues.intake.queue.next", {
                            date: formatDate(queued.next_attempt_at),
                          })
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
                        {t("issues.intake.queue.proposal")}{" "}
                        <span className="mono">
                          {shortId(queued.intake_request_id)}
                        </span>{" "}
                        · {t("issues.intake.queue.lead")}{" "}
                        <span className="mono">
                          {shortId(queued.lead_request_id ?? "")}
                        </span>{" "}
                        · {t("issues.intake.queue.noCampaign")}
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
                          ? t("issues.intake.checkingFreshness")
                          : t("issues.intake.createProposal")}
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
          <summary>
            {t("issues.intake.audit.title", { count: state.audit.length })}
          </summary>
          <ul className="issues-audit">
            {state.audit.map((entry) => (
              <li key={entry.audit_id}>
                <strong>{entry.action}</strong>{" "}
                {t("issues.intake.audit.by", { actor: entry.actor })} —{" "}
                {formatDate(entry.created_at)}
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
