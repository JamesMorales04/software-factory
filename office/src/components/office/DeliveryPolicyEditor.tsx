import { RefreshCw, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import {
  CoreApiError,
  coreApi,
  type DeliveryPolicy,
  type ProjectDeliveryPolicyView,
} from "../../coreApi";
import { formatTimestamp } from "../../format";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { Checkbox, Input } from "../ui/Field";
import { ErrorState, LoadingState, Panel } from "../ui/Surface";
import { StatusBadge } from "./StatusBadge";

function policyError(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function formFromPolicy(policy: DeliveryPolicy | null) {
  return {
    enabled: policy?.mode === "RISK_TIERED",
    r1Enabled: Boolean(policy?.r1Enabled),
    checks: (policy?.requiredChecks ?? []).join(", "),
    postMerge: policy?.postMergeCheckId ?? "post-merge",
    timeout: String(policy?.postMergeTimeoutSecs ?? 900),
    killSwitch: Boolean(policy?.killSwitch),
    expiresAt: policy?.expiresAt ?? "",
  };
}

export function DeliveryPolicyEditor({ projectId }: { projectId: string }) {
  const { t } = useI18n();
  const [view, setView] = useState<ProjectDeliveryPolicyView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [form, setForm] = useState(formFromPolicy(null));

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await coreApi.getProjectDeliveryPolicy(projectId);
      setView(next);
      setForm(formFromPolicy(next.policy));
    } catch (cause) {
      setError(policyError(cause, t("deliveryPolicy.error")));
    } finally {
      setLoading(false);
    }
  }, [projectId, t]);

  useEffect(() => {
    void load();
  }, [load]);

  async function save() {
    setSaving(true);
    setActionError(null);
    setNotice(null);
    try {
      const result = await coreApi.updateProjectDeliveryPolicy(projectId, {
        enabled: form.enabled,
        r1_enabled: form.r1Enabled,
        merge_method: "squash",
        required_checks: form.checks
          .split(",")
          .map((value) => value.trim())
          .filter(Boolean),
        post_merge_check_id: form.postMerge.trim(),
        post_merge_timeout_secs: Number(form.timeout),
        kill_switch: form.killSwitch,
        expires_at: form.expiresAt.trim() || null,
        manual_base_branches: result_default_bases(view),
      });
      setView((current) =>
        current
          ? {
              ...current,
              policy: result.policy,
              revisions: result.created
                ? [result.policy, ...current.revisions]
                : current.revisions,
            }
          : current,
      );
      setForm(formFromPolicy(result.policy));
      setNotice(t("deliveryPolicy.saved"));
    } catch (cause) {
      setActionError(policyError(cause, t("deliveryPolicy.error")));
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return (
      <Panel className="delivery-policy-editor">
        <LoadingState label={t("deliveryPolicy.loading")} />
      </Panel>
    );
  }
  if (error || !view) {
    return (
      <Panel className="delivery-policy-editor">
        <ErrorState
          title={t("deliveryPolicy.title")}
          message={error ?? t("deliveryPolicy.error")}
          action={
            <Button variant="secondary" onClick={() => void load()}>
              <RefreshCw size={15} aria-hidden="true" />
              {t("common.retry")}
            </Button>
          }
        />
      </Panel>
    );
  }

  if (!view.binding) {
    return (
      <Panel
        className="delivery-policy-editor"
        data-testid="delivery-policy-editor"
      >
        <h2>{t("deliveryPolicy.title")}</h2>
        <p className="warning">{t("deliveryPolicy.unbound")}</p>
      </Panel>
    );
  }

  return (
    <Panel
      className="delivery-policy-editor"
      data-testid="delivery-policy-editor"
    >
      <div className="delivery-head">
        <div>
          <h2>{t("deliveryPolicy.title")}</h2>
          <p className="muted">{t("deliveryPolicy.description")}</p>
        </div>
        <StatusBadge
          status={view.policy?.mode === "RISK_TIERED" ? "ENABLED" : "DISABLED"}
          label={
            view.policy?.mode === "RISK_TIERED"
              ? t("delivery.policyEnabled")
              : t("delivery.policyDisabled")
          }
        />
      </div>
      <p className="muted">
        {view.policy
          ? t("deliveryPolicy.currentRevision", {
              revision: view.policy.revision,
            })
          : t("deliveryPolicy.noPolicy")}
        {view.policy
          ? ` · ${formatTimestamp(view.policy.createdAt)} · ${view.policy.createdBy}`
          : ""}
      </p>
      <div className="delivery-policy-form">
        <Checkbox
          checked={form.enabled}
          onChange={(event) =>
            setForm((current) => ({
              ...current,
              enabled: event.target.checked,
            }))
          }
          label={t("deliveryPolicy.enabled")}
        />
        <Checkbox
          checked={form.r1Enabled}
          disabled={!form.enabled}
          onChange={(event) =>
            setForm((current) => ({
              ...current,
              r1Enabled: event.target.checked,
            }))
          }
          label={t("deliveryPolicy.r1")}
        />
        <Input
          label={t("deliveryPolicy.mergeMethod")}
          value={t("deliveryPolicy.squash")}
          readOnly
        />
        <Input
          label={t("deliveryPolicy.checks")}
          help={t("deliveryPolicy.checksHint")}
          value={form.checks}
          onChange={(event) =>
            setForm((current) => ({ ...current, checks: event.target.value }))
          }
        />
        <Input
          label={t("deliveryPolicy.postMerge")}
          help={t("deliveryPolicy.postMergeHint")}
          value={form.postMerge}
          onChange={(event) =>
            setForm((current) => ({
              ...current,
              postMerge: event.target.value,
            }))
          }
        />
        <Input
          label={t("deliveryPolicy.timeout")}
          type="number"
          min={1}
          max={1800}
          value={form.timeout}
          onChange={(event) =>
            setForm((current) => ({ ...current, timeout: event.target.value }))
          }
        />
        <Input
          label={t("deliveryPolicy.expiresAt")}
          value={form.expiresAt}
          onChange={(event) =>
            setForm((current) => ({
              ...current,
              expiresAt: event.target.value,
            }))
          }
        />
        <Checkbox
          checked={form.killSwitch}
          onChange={(event) =>
            setForm((current) => ({
              ...current,
              killSwitch: event.target.checked,
            }))
          }
          label={t("deliveryPolicy.killSwitch")}
        />
      </div>
      {actionError && (
        <p className="ui-error" role="alert">
          {actionError}
        </p>
      )}
      {notice && <p className="notice">{notice}</p>}
      <div className="delivery-actions">
        <Button
          variant="primary"
          busy={saving}
          disabled={saving}
          onClick={() => void save()}
        >
          <ShieldCheck size={16} aria-hidden="true" />
          {saving ? t("deliveryPolicy.saving") : t("deliveryPolicy.save")}
        </Button>
      </div>
      <p className="muted">{t("deliveryPolicy.ownerOnly")}</p>
      <p className="muted">
        {t("deliveryPolicy.manualBase")}: {view.binding.default_branch}
      </p>
    </Panel>
  );
}

function result_default_bases(
  view: ProjectDeliveryPolicyView | null,
): string[] {
  if (!view?.binding) return [];
  const policy = view.policy;
  if (policy && policy.manualBaseBranches.length > 0) {
    return policy.manualBaseBranches;
  }
  return [view.binding.default_branch];
}
