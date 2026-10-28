import { Badge } from "../components/ui/Badge";
import { EmptyState, Panel } from "../components/ui/Surface";
import type {
  FrozenLeadPlanRecord,
  LeadPlanProposalView,
  LeadWorkUnitView,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";

export function RequestPlan({
  proposal,
  plan,
}: {
  proposal: LeadPlanProposalView | null;
  plan: FrozenLeadPlanRecord | null;
}) {
  const { t, formatDate } = useI18n();

  function valuesText(values: string[]): string {
    return values.length === 0
      ? t("request.values.noneRecorded")
      : values.join("; ");
  }

  function checksSummary(unit: LeadWorkUnitView): string {
    return t("request.plan.checksValue", {
      acceptance: unit.acceptanceCriteria.length,
      negative: unit.negativeCases.length,
    });
  }

  return (
    <Panel className="request-panel" as="section">
      <div className="section-heading">
        <h2>{t("request.plan.title")}</h2>
        {plan ? (
          <Badge tone="success">{t("request.plan.frozen")}</Badge>
        ) : proposal ? (
          <Badge tone="attention">{t("request.plan.notFrozen")}</Badge>
        ) : (
          <span className="muted">{t("request.plan.none")}</span>
        )}
      </div>

      {plan ? (
        <div className="request-frozen-plan">
          <p className="muted">{t("request.plan.description")}</p>
          <dl className="request-facts">
            <div>
              <dt>{t("request.plan.planHash")}</dt>
              <dd>
                <code title={plan.plan_hash}>{plan.plan_hash}</code>
              </dd>
            </div>
            <div>
              <dt>{t("request.plan.configHash")}</dt>
              <dd>
                <code title={plan.config_hash}>{plan.config_hash}</code>
              </dd>
            </div>
            <div>
              <dt>{t("request.plan.delegationHash")}</dt>
              <dd>
                <code title={plan.delegation_hash}>{plan.delegation_hash}</code>
              </dd>
            </div>
            <div>
              <dt>{t("request.plan.proposal")}</dt>
              <dd>
                <code>{shortId(plan.proposal_id)}</code>
              </dd>
            </div>
            <div>
              <dt>{t("request.plan.frozenAt")}</dt>
              <dd>{formatDate(plan.created_at)}</dd>
            </div>
          </dl>
        </div>
      ) : proposal ? (
        <p className="ui-callout ui-callout--attention">
          {t("request.plan.notFrozenDescription")}
        </p>
      ) : (
        <EmptyState
          title={t("request.plan.emptyTitle")}
          description={t("request.plan.emptyDescription")}
        />
      )}

      {proposal && (
        <div className="request-proposal">
          <div className="section-heading request-proposal-heading">
            <h3>
              {plan
                ? t("request.plan.frozenContent")
                : t("request.plan.proposedPlan")}
            </h3>
            <span className="muted">
              {t(
                proposal.workUnits.length === 1
                  ? "request.plan.workUnitOne"
                  : "request.plan.workUnitOther",
                { count: proposal.workUnits.length },
              )}
            </span>
          </div>
          <dl className="request-facts">
            <div>
              <dt>{t("request.plan.objective")}</dt>
              <dd>{proposal.objective}</dd>
            </div>
            <div>
              <dt>{t("request.plan.classRisk")}</dt>
              <dd>
                {proposal.class} / {proposal.riskClass}
              </dd>
            </div>
            <div>
              <dt>{t("request.plan.qualityProfile")}</dt>
              <dd>{proposal.qualityProfile}</dd>
            </div>
            <div>
              <dt>{t("request.plan.route")}</dt>
              <dd>
                <code>
                  {proposal.route.provider}/{proposal.route.model}
                </code>{" "}
                ({proposal.route.mode})
              </dd>
            </div>
            <div>
              <dt>{t("request.plan.budget")}</dt>
              <dd>
                {t("request.plan.budgetValue", {
                  attempts: proposal.budget.maxAttempts,
                  timeout: proposal.budget.timeoutSecs,
                })}
              </dd>
            </div>
            <div>
              <dt>{t("request.plan.bounds")}</dt>
              <dd>
                {t("request.plan.boundsValue", {
                  sessions: proposal.maxSessions,
                  parallel: proposal.maxParallel,
                  wallClock: proposal.maxWallClockSecs,
                  repairCycles: proposal.maxRepairCycles,
                })}
              </dd>
            </div>
            <div>
              <dt>{t("request.plan.checks")}</dt>
              <dd>
                {t("request.plan.checksValue", {
                  acceptance: proposal.acceptanceCriteria.length,
                  negative: proposal.negativeCases.length,
                })}
              </dd>
            </div>
          </dl>
          <dl className="request-facts request-facts--wide">
            <div>
              <dt>{t("request.plan.assumptions")}</dt>
              <dd>{valuesText(proposal.assumptions)}</dd>
            </div>
            <div>
              <dt>{t("request.plan.risks")}</dt>
              <dd>{valuesText(proposal.risks)}</dd>
            </div>
          </dl>

          <h4 className="request-workunits-heading">
            {t("request.plan.workUnits")}
          </h4>
          {proposal.workUnits.length === 0 ? (
            <p className="muted">{t("request.plan.noWorkUnits")}</p>
          ) : (
            <ol className="request-workunits">
              {proposal.workUnits.map((unit) => (
                <li key={unit.id}>
                  <div className="request-workunit-head">
                    <code>{unit.id}</code>
                    <span className="muted">{unit.kind}</span>
                  </div>
                  <p>{unit.description}</p>
                  <dl className="request-workunit-facts">
                    <div>
                      <dt>{t("request.plan.dependsOn")}</dt>
                      <dd>{valuesText(unit.dependsOn)}</dd>
                    </div>
                    <div>
                      <dt>{t("request.plan.allowedPaths")}</dt>
                      <dd>{valuesText(unit.allowedPaths)}</dd>
                    </div>
                    <div>
                      <dt>{t("request.plan.capabilities")}</dt>
                      <dd>{valuesText(unit.capabilities)}</dd>
                    </div>
                    <div>
                      <dt>{t("request.plan.checks")}</dt>
                      <dd>{checksSummary(unit)}</dd>
                    </div>
                  </dl>
                </li>
              ))}
            </ol>
          )}
        </div>
      )}
    </Panel>
  );
}
