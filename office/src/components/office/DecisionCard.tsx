import { useEffect, useId, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  CoreApiError,
  coreApi,
  type DecisionAction,
  type DecisionBudget,
  type DecisionItem,
} from "../../coreApi";
import { shortId } from "../../format";
import { type MessageKey, type Translate, useI18n } from "../../i18n";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Checkbox, Input } from "../ui/Field";
import { Panel } from "../ui/Surface";
import { ModelCallResolution } from "./ModelCallResolution";
import { StatusBadge } from "./StatusBadge";

const TARGET_LABEL_KEYS: Record<string, MessageKey> = {
  "campaign.authorization": "decisions.target.campaign",
  "lead.ready": "decisions.target.lead",
  "intake.plan-approval": "decisions.target.request",
  "quality.gate": "decisions.target.campaign",
  "run.failed": "decisions.target.campaign",
  "model.usage-unresolved": "decisions.target.runtime",
  "provider.unresolved-session": "decisions.target.runtime",
  "action.human-required": "decisions.target.action",
};

const STALE_CODES = new Set([
  "PLAN_HASH_MISMATCH",
  "PROFILE_REVISION_MISMATCH",
  "PROFILE_HASH_MISMATCH",
  "EXECUTION_PROFILE_STALE",
  "INTAKE_PROFILE_CONTEXT_MISMATCH",
  "INTAKE_WITHDRAWN",
  "LEAD_ALREADY_STARTED",
  "PROJECT_CONFIG_CHANGED",
  "EXECUTION_POLICY_REQUIRED",
  "EXECUTION_POLICY_SCOPE_CHANGED",
  "DELEGATION_REVOKED",
  "DELEGATION_EXPIRED",
]);

export type DecisionRevalidation =
  | { status: "current"; item: DecisionItem }
  | { status: "changed"; item: DecisionItem }
  | { status: "missing" };

export type RevalidateDecision = (
  id: string,
  revision: string | undefined,
) => Promise<DecisionRevalidation>;

function targetLabel(item: DecisionItem, t: Translate): string {
  const key = TARGET_LABEL_KEYS[item.kind];
  return key ? t(key) : t("decisions.target.default");
}

function toneForDecision(severity: DecisionItem["severity"]) {
  if (severity === "critical") return "danger" as const;
  if (severity === "attention") return "attention" as const;
  return "info" as const;
}

function severityKey(severity: DecisionItem["severity"]): MessageKey {
  if (severity === "critical") return "decisions.severity.critical";
  if (severity === "attention") return "decisions.severity.attention";
  return "decisions.severity.info";
}

function riskTone(riskClass: string | null | undefined) {
  if (riskClass === "R1") return "attention" as const;
  if (riskClass === "R2" || riskClass === "R3") return "danger" as const;
  if (riskClass === "R0") return "info" as const;
  return "neutral" as const;
}

export function decisionActions(item: DecisionItem): DecisionAction[] {
  if (item.actions && item.actions.length > 0) return item.actions;
  return item.action ? [item.action] : [];
}

function limit(
  value: number | undefined,
  t: Translate,
  formatNumber: (value: number) => string,
): string {
  return value !== undefined && value > 0
    ? formatNumber(value)
    : t("common.notSet");
}

function BudgetFacts({ budget }: { budget: DecisionBudget }) {
  const { t, formatNumber, formatMicros } = useI18n();
  const facts: Array<[MessageKey, string]> = [];
  if (budget.max_attempts !== undefined) {
    facts.push([
      "decisions.budget.attempts",
      limit(budget.max_attempts, t, formatNumber),
    ]);
  }
  if (budget.max_sessions !== undefined) {
    facts.push([
      "decisions.budget.sessions",
      limit(budget.max_sessions, t, formatNumber),
    ]);
  }
  if (budget.max_turns !== undefined) {
    facts.push([
      "decisions.budget.turns",
      limit(budget.max_turns, t, formatNumber),
    ]);
  }
  if (budget.max_input_tokens !== undefined) {
    facts.push([
      "decisions.budget.inputTokens",
      limit(budget.max_input_tokens, t, formatNumber),
    ]);
  }
  if (budget.max_output_tokens !== undefined) {
    facts.push([
      "decisions.budget.outputTokens",
      limit(budget.max_output_tokens, t, formatNumber),
    ]);
  }
  if (budget.max_cost_micros !== undefined) {
    facts.push([
      "decisions.budget.cost",
      budget.max_cost_micros > 0
        ? formatMicros(budget.max_cost_micros)
        : t("common.notSet"),
    ]);
  }
  if (budget.deadline_secs !== undefined) {
    facts.push([
      "decisions.budget.deadline",
      budget.deadline_secs > 0
        ? `${formatNumber(budget.deadline_secs)} s`
        : t("common.notSet"),
    ]);
  }
  if (budget.max_wall_clock_secs !== undefined) {
    facts.push([
      "decisions.budget.wallClock",
      budget.max_wall_clock_secs > 0
        ? `${formatNumber(budget.max_wall_clock_secs)} s`
        : t("common.notSet"),
    ]);
  }
  if (budget.timeout_secs !== undefined) {
    facts.push([
      "decisions.budget.timeout",
      budget.timeout_secs > 0
        ? `${formatNumber(budget.timeout_secs)} s`
        : t("common.notSet"),
    ]);
  }
  if (budget.max_repair_cycles !== undefined) {
    facts.push([
      "decisions.budget.repairCycles",
      limit(budget.max_repair_cycles, t, formatNumber),
    ]);
  }
  if (budget.max_parallel !== undefined) {
    facts.push([
      "decisions.budget.parallel",
      limit(budget.max_parallel, t, formatNumber),
    ]);
  }
  if (budget.usage_known !== undefined) {
    facts.push([
      "decisions.budget.usage",
      budget.usage_known ? t("decisions.measured") : t("decisions.unmeasured"),
    ]);
  }
  if (budget.cost_micros !== undefined) {
    facts.push([
      "decisions.budget.recordedCost",
      formatMicros(budget.cost_micros),
    ]);
  }
  if (facts.length === 0) {
    return <p className="muted">{t("decisions.noBudget")}</p>;
  }
  return (
    <dl className="decision-fact-list">
      {facts.map(([key, value]) => (
        <div key={key}>
          <dt>{t(key)}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function actionConfig(action: DecisionAction, t: Translate) {
  switch (action.kind) {
    case "approve_intake_plan":
      return {
        trigger: t("decisions.action.approveIntake"),
        submit: t("decisions.action.approveIntakeSubmit"),
        success: t("decisions.action.approved"),
        consequence: t("decisions.action.approveIntakeConsequence"),
        textLabel: t("decisions.action.rationale"),
        textHelp: t("decisions.action.rationaleHelp"),
        confirm: t("decisions.action.confirmApprove"),
        requiredText: t("decisions.action.requiredRationale"),
      };
    case "cancel_intake":
      return {
        trigger: t("decisions.action.cancelIntake"),
        submit: t("decisions.action.cancelIntakeSubmit"),
        success: t("decisions.action.intakeCancelled"),
        consequence: t("decisions.action.cancelIntakeConsequence"),
        textLabel: t("decisions.action.reason"),
        textHelp: t("decisions.action.reasonHelp"),
        confirm: t("decisions.action.confirmWithdraw"),
        requiredText: t("decisions.action.requiredReason"),
      };
    case "withdraw_lead":
      return {
        trigger: t("decisions.action.withdrawLead"),
        submit: t("decisions.action.withdrawLeadSubmit"),
        success: t("decisions.action.leadWithdrawn"),
        consequence: t("decisions.action.withdrawLeadConsequence"),
        textLabel: t("decisions.action.reason"),
        textHelp: t("decisions.action.reasonHelp"),
        confirm: t("decisions.action.confirmWithdraw"),
        requiredText: t("decisions.action.requiredReason"),
      };
    case "cancel_campaign":
      return {
        trigger: t("decisions.action.cancelCampaign"),
        submit: t("decisions.action.cancelCampaignSubmit"),
        success: t("decisions.action.campaignCancelled"),
        consequence: t("decisions.action.cancelCampaignConsequence"),
        textLabel: t("decisions.action.reason"),
        textHelp: t("decisions.action.reasonHelp"),
        confirm: t("decisions.action.confirmCampaignCancel"),
        requiredText: t("decisions.action.requiredReason"),
      };
    default:
      return {
        trigger: action.label,
        submit: action.label,
        success: t("decisions.action.completed"),
        consequence: t("decisions.action.noConsequence"),
        textLabel: t("decisions.action.reason"),
        textHelp: t("decisions.action.reasonHelp"),
        confirm: t("decisions.action.confirmWithdraw"),
        requiredText: t("decisions.action.requiredReason"),
      };
  }
}

function staleMessage(cause: CoreApiError, t: Translate): string {
  if (cause.code === "LEAD_ALREADY_STARTED") {
    const body =
      typeof cause.body === "object" && cause.body !== null
        ? (cause.body as { campaign_id?: unknown })
        : null;
    const campaignId =
      typeof body?.campaign_id === "string" ? body.campaign_id : null;
    return campaignId
      ? `${t("decisions.action.alreadyStarted")} (${shortId(campaignId)})`
      : t("decisions.action.alreadyStarted");
  }
  return t("decisions.staleBody");
}

function QuickAction({
  item,
  action,
  offline,
  revalidate,
  onMutated,
  onStale,
  onOffline,
}: {
  item: DecisionItem;
  action: DecisionAction;
  offline: boolean;
  revalidate: RevalidateDecision;
  onMutated: (message: string) => Promise<void>;
  onStale: (message: string) => void;
  onOffline: () => void;
}) {
  const { t } = useI18n();

  if (
    ![
      "resolve_model_call",
      "approve_intake_plan",
      "cancel_intake",
      "withdraw_lead",
      "cancel_campaign",
    ].includes(action.kind)
  ) {
    return (
      <p className="muted">
        {t("decisions.action.unsupported")} <code>{action.kind}</code>
      </p>
    );
  }

  if (action.kind === "resolve_model_call") {
    if (offline) {
      return (
        <p className="ui-callout ui-callout--attention">
          {t("decisions.offlineActions")}
        </p>
      );
    }
    return (
      <ModelCallResolution
        modelCallId={action.model_call_id ?? item.subject_id}
        onResolved={() => onMutated(t("decisions.action.modelResolved"))}
      />
    );
  }

  return (
    <DisclosureAction
      item={item}
      action={action}
      offline={offline}
      revalidate={revalidate}
      onMutated={onMutated}
      onStale={onStale}
      onOffline={onOffline}
    />
  );
}

function DisclosureAction({
  item,
  action,
  offline,
  revalidate,
  onMutated,
  onStale,
  onOffline,
}: {
  item: DecisionItem;
  action: DecisionAction;
  offline: boolean;
  revalidate: RevalidateDecision;
  onMutated: (message: string) => Promise<void>;
  onStale: (message: string) => void;
  onOffline: () => void;
}) {
  const { t } = useI18n();
  const panelId = useId();
  const fieldId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const inFlight = useRef(false);
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const config = actionConfig(action, t);
  const approval = action.kind === "approve_intake_plan";

  useEffect(() => {
    if (open) {
      window.setTimeout(() => document.getElementById(fieldId)?.focus(), 0);
    }
  }, [fieldId, open]);

  function close() {
    setOpen(false);
    setError(null);
    setConfirmed(false);
    triggerRef.current?.focus();
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (inFlight.current || busy || offline) return;
    if (text.trim() === "") {
      setError(config.requiredText);
      document.getElementById(fieldId)?.focus();
      return;
    }
    if (!confirmed) {
      setError(t("decisions.action.requiredConfirmation"));
      return;
    }
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      const current = await revalidate(item.id, item.revision);
      if (current.status !== "current") {
        onStale(
          current.status === "missing"
            ? t("decisions.action.staleMissing")
            : t("decisions.staleBody"),
        );
        return;
      }
      switch (action.kind) {
        case "approve_intake_plan":
          if (!action.intake_id || !action.expected_plan_hash) {
            throw new Error(t("decisions.action.error"));
          }
          await coreApi.approveIntakeRequest(action.intake_id, {
            expected_plan_hash: action.expected_plan_hash,
            ...(action.expected_profile_revision_id
              ? {
                  expected_profile_revision_id:
                    action.expected_profile_revision_id,
                }
              : {}),
            ...(action.expected_profile_hash
              ? { expected_profile_hash: action.expected_profile_hash }
              : {}),
            rationale: text.trim(),
          });
          break;
        case "cancel_intake":
          if (!action.intake_id) throw new Error(t("decisions.action.error"));
          await coreApi.cancelIntakeRequest(action.intake_id, text.trim());
          break;
        case "withdraw_lead":
          if (!action.lead_request_id) {
            throw new Error(t("decisions.action.error"));
          }
          await coreApi.withdrawLeadRequest(
            action.lead_request_id,
            text.trim(),
          );
          break;
        case "cancel_campaign":
          if (!action.campaign_id) throw new Error(t("decisions.action.error"));
          await coreApi.cancelCampaign(action.campaign_id, text.trim());
          break;
        default:
          throw new Error(t("decisions.action.error"));
      }
      setText("");
      setConfirmed(false);
      setOpen(false);
      await onMutated(config.success);
    } catch (cause) {
      if (!(cause instanceof CoreApiError)) {
        setError(t("decisions.action.offline"));
        onOffline();
        return;
      }
      if (cause.status === 401 || cause.status === 403) {
        setError(t("decisions.action.unauthorized"));
        return;
      }
      if (
        cause.status === 409 &&
        (!cause.code || STALE_CODES.has(cause.code))
      ) {
        setError(staleMessage(cause, t));
        onStale(staleMessage(cause, t));
        return;
      }
      if (cause.code && STALE_CODES.has(cause.code)) {
        setError(staleMessage(cause, t));
        onStale(staleMessage(cause, t));
        return;
      }
      setError(cause.message || t("decisions.action.error"));
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }

  return (
    <div className="decision-quick-action">
      <Button
        ref={triggerRef}
        variant={
          action.kind === "approve_intake_plan" ? "primary" : "secondary"
        }
        size="sm"
        className="decision-quick-action-trigger"
        aria-expanded={open}
        aria-controls={open ? panelId : undefined}
        disabled={offline || busy}
        title={offline ? t("decisions.offlineActions") : undefined}
        onClick={() => {
          if (open) {
            close();
          } else {
            setError(null);
            setOpen(true);
          }
        }}
      >
        {config.trigger}
      </Button>
      {open && (
        <form
          id={panelId}
          className="decision-quick-action-panel"
          onSubmit={(event) => void submit(event)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.stopPropagation();
              close();
            }
          }}
          aria-busy={busy}
        >
          <p className="ui-callout ui-callout--attention">
            {config.consequence}
          </p>
          {approval && (
            <dl className="decision-approval-identity">
              <div>
                <dt>{t("decisions.action.planHash")}</dt>
                <dd>
                  <code title={action.expected_plan_hash}>
                    {action.expected_plan_hash ?? t("common.notRecorded")}
                  </code>
                </dd>
              </div>
              <div>
                <dt>{t("decisions.action.profileRevision")}</dt>
                <dd>
                  <code title={action.expected_profile_revision_id}>
                    {action.expected_profile_revision_id ??
                      t("common.notRecorded")}
                  </code>
                </dd>
              </div>
              <div>
                <dt>{t("decisions.action.profileHash")}</dt>
                <dd>
                  <code title={action.expected_profile_hash}>
                    {action.expected_profile_hash ?? t("common.notRecorded")}
                  </code>
                </dd>
              </div>
            </dl>
          )}
          <Input
            id={fieldId}
            autoFocus
            label={config.textLabel}
            help={config.textHelp}
            value={text}
            onChange={(event) => setText(event.target.value)}
            maxLength={
              action.kind === "approve_intake_plan"
                ? 2000
                : action.kind === "cancel_campaign"
                  ? 500
                  : 1000
            }
            required
          />
          <Checkbox
            label={config.confirm}
            checked={confirmed}
            onChange={(event) => setConfirmed(event.target.checked)}
          />
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
          <div className="decision-quick-action-buttons">
            <Button
              type="submit"
              variant={
                action.kind === "approve_intake_plan" ? "primary" : "danger"
              }
              size="sm"
              disabled={offline || busy || !confirmed || text.trim() === ""}
            >
              {busy ? t("decisions.action.busy") : config.submit}
            </Button>
            <Button variant="quiet" size="sm" disabled={busy} onClick={close}>
              {t("decisions.action.close")}
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}

export function DecisionCard({
  item,
  offline,
  revalidate,
  onMutated,
  onStale,
  onOffline,
}: {
  item: DecisionItem;
  offline: boolean;
  revalidate: RevalidateDecision;
  onMutated: (message: string) => Promise<void>;
  onStale: (message: string) => void;
  onOffline: () => void;
}) {
  const { t, formatDate, formatNumber } = useI18n();
  const actions = decisionActions(item);
  const [staleRevision, setStaleRevision] = useState<string | undefined>();
  const stale = staleRevision !== undefined && staleRevision === item.revision;
  const project = item.project;
  const campaign = item.campaign;
  const change = item.change;
  const risk = item.risk;
  const impact = item.impact;
  const evidence = item.evidence ?? [];
  const changeId = useId();
  const impactId = useId();

  return (
    <Panel className="decision-item" data-testid={`decision-item-${item.id}`}>
      <div className="decision-item-head">
        <Badge tone={toneForDecision(item.severity)}>
          {t(severityKey(item.severity))}
        </Badge>
        <h2>{item.title}</h2>
        <span className="muted">{formatDate(item.updated_at)}</span>
      </div>
      <p className="decision-card-detail">{item.detail}</p>

      {(project || campaign || risk) && (
        <section
          className="decision-context"
          aria-label={t("decisions.context")}
        >
          {project && (
            <div className="decision-context-item">
              <span>{t("decisions.project")}</span>
              <code title={project.repository ?? project.id}>
                {project.repository
                  ? `${project.repository} · ${shortId(project.id)}`
                  : shortId(project.id)}
              </code>
            </div>
          )}
          {campaign && (
            <div className="decision-context-item">
              <span>{t("decisions.campaign")}</span>
              <Link to={campaign.target}>
                <code>{shortId(campaign.id)}</code>
              </Link>
              <StatusBadge status={campaign.status} />
            </div>
          )}
          {risk && (
            <div className="decision-context-item">
              <span>{t("decisions.risk")}</span>
              <Badge tone={riskTone(risk.class)}>
                {risk.class ?? t("common.notRecorded")}
              </Badge>
              <span className="muted">{risk.summary}</span>
            </div>
          )}
        </section>
      )}

      {change && (
        <section className="decision-block" aria-labelledby={changeId}>
          <h3 id={changeId}>{t("decisions.change")}</h3>
          <p>{change.summary}</p>
          <dl className="decision-inline-facts">
            {change.work_units !== undefined && (
              <div>
                <dt>{t("decisions.workUnits")}</dt>
                <dd>{formatNumber(change.work_units)}</dd>
              </div>
            )}
            {change.acceptance_criteria !== undefined && (
              <div>
                <dt>{t("decisions.acceptanceCriteria")}</dt>
                <dd>{formatNumber(change.acceptance_criteria)}</dd>
              </div>
            )}
            {change.negative_cases !== undefined && (
              <div>
                <dt>{t("decisions.negativeCases")}</dt>
                <dd>{formatNumber(change.negative_cases)}</dd>
              </div>
            )}
            {change.paths && change.paths.length > 0 && (
              <div>
                <dt>{t("decisions.paths")}</dt>
                <dd>
                  {change.paths.map((path) => (
                    <code key={path}>{path}</code>
                  ))}
                </dd>
              </div>
            )}
          </dl>
        </section>
      )}

      {impact && (
        <section className="decision-impact" aria-labelledby={impactId}>
          <h3 id={impactId}>{t("decisions.impact")}</h3>
          <div className="decision-impact-grid">
            <div className="decision-impact-section">
              <h4>{t("decisions.budget")}</h4>
              {impact.budget ? (
                <BudgetFacts budget={impact.budget} />
              ) : (
                <p className="muted">{t("decisions.noBudget")}</p>
              )}
            </div>
            <div className="decision-impact-section">
              <h4>{t("decisions.permissions")}</h4>
              {impact.permissions ? (
                <dl className="decision-fact-list">
                  {impact.permissions.execution_mode && (
                    <div>
                      <dt>{t("decisions.executionMode")}</dt>
                      <dd>
                        <code>{impact.permissions.execution_mode}</code>
                      </dd>
                    </div>
                  )}
                  {impact.permissions.route && (
                    <div>
                      <dt>{t("decisions.route")}</dt>
                      <dd>
                        <code>
                          {impact.permissions.route.provider}/
                          {impact.permissions.route.model} (
                          {impact.permissions.route.mode})
                        </code>
                      </dd>
                    </div>
                  )}
                  {impact.permissions.allowed_paths && (
                    <div>
                      <dt>{t("decisions.paths")}</dt>
                      <dd>
                        {impact.permissions.allowed_paths.length > 0
                          ? impact.permissions.allowed_paths.map((path) => (
                              <code key={path}>{path}</code>
                            ))
                          : t("common.notSet")}
                      </dd>
                    </div>
                  )}
                  {impact.permissions.allowed_tools && (
                    <div>
                      <dt>{t("decisions.tools")}</dt>
                      <dd>
                        {impact.permissions.allowed_tools.length > 0
                          ? impact.permissions.allowed_tools.join(", ")
                          : t("common.notSet")}
                      </dd>
                    </div>
                  )}
                  {impact.permissions.delegation_id && (
                    <div>
                      <dt>{t("decisions.delegation")}</dt>
                      <dd>
                        <code title={impact.permissions.delegation_id}>
                          {shortId(impact.permissions.delegation_id, 16)}
                        </code>
                      </dd>
                    </div>
                  )}
                </dl>
              ) : (
                <p className="muted">{t("decisions.noPermissions")}</p>
              )}
            </div>
          </div>
          {impact.note && (
            <p className="muted decision-impact-note">{impact.note}</p>
          )}
        </section>
      )}

      {item.blocked_reason && (
        <div className="ui-callout ui-callout--attention" role="status">
          <strong>{t("decisions.actionUnavailable")}</strong>{" "}
          <code>{item.blocked_reason}</code>
          <p>{t("decisions.actionUnavailableDetail")}</p>
        </div>
      )}

      {evidence.length > 0 && (
        <dl
          className="decision-inline-facts decision-evidence-summary"
          aria-label={t("decisions.evidenceSummary")}
        >
          {evidence.slice(0, 3).map((entry) => (
            <div key={entry.reference ?? `${entry.label}:${entry.value}`}>
              <dt>{entry.label}</dt>
              <dd title={entry.value}>
                <code>{shortId(entry.value, 32)}</code>
              </dd>
            </div>
          ))}
        </dl>
      )}

      {evidence.length > 0 && (
        <details className="decision-evidence">
          <summary>
            {t("decisions.evidence")} ({formatNumber(evidence.length)})
          </summary>
          <dl className="decision-evidence-list">
            {evidence.map((entry) => (
              <div key={entry.reference ?? `${entry.label}:${entry.value}`}>
                <dt>{entry.label}</dt>
                <dd>
                  <code title={entry.value}>{entry.value}</code>
                </dd>
              </div>
            ))}
          </dl>
        </details>
      )}

      <div className="decision-item-actions">
        <Link
          className="ui-button ui-button--secondary ui-button--sm"
          to={item.target}
        >
          {targetLabel(item, t)}
        </Link>
        <span className="muted">
          {t("decisions.subject", { id: item.subject_id })}
        </span>
        {item.revision && (
          <span className="muted decision-revision">
            {t("decisions.revision")}{" "}
            <code title={item.revision}>{shortId(item.revision, 12)}</code>
          </span>
        )}
      </div>

      {stale && (
        <div className="ui-callout ui-callout--attention" role="alert">
          <strong>{t("decisions.staleTitle")}</strong>{" "}
          {t("decisions.staleBody")}
          <div className="decision-stale-actions">
            <Button
              size="sm"
              onClick={() => {
                setStaleRevision(undefined);
                void onMutated(t("decisions.staleBody"));
              }}
            >
              {t("decisions.refresh")}
            </Button>
          </div>
        </div>
      )}

      {item.actionable && !stale && (
        <fieldset
          key={item.revision ?? item.id}
          className="decision-quick-actions"
        >
          <legend>{t("decisions.actions")}</legend>
          {actions.length === 0 ? (
            <p className="muted">{t("decisions.noActions")}</p>
          ) : (
            actions.map((action, index) => (
              <QuickAction
                key={`${action.kind}:${action.model_call_id ?? action.intake_id ?? action.lead_request_id ?? action.campaign_id ?? index}`}
                item={item}
                action={action}
                offline={offline}
                revalidate={revalidate}
                onMutated={onMutated}
                onStale={(message) => {
                  setStaleRevision(item.revision);
                  onStale(message);
                }}
                onOffline={onOffline}
              />
            ))
          )}
        </fieldset>
      )}
    </Panel>
  );
}
