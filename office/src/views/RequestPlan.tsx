import { Badge } from "../components/ui/Badge";
import { EmptyState, Panel } from "../components/ui/Surface";
import type {
  FrozenLeadPlanRecord,
  LeadPlanProposalView,
  LeadWorkUnitView,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

function valuesText(values: string[]): string {
  return values.length === 0 ? "None recorded" : values.join("; ");
}

function checksSummary(unit: LeadWorkUnitView): string {
  return `${unit.acceptanceCriteria.length} acceptance · ${unit.negativeCases.length} negative`;
}

export function RequestPlan({
  proposal,
  plan,
}: {
  proposal: LeadPlanProposalView | null;
  plan: FrozenLeadPlanRecord | null;
}) {
  return (
    <Panel className="request-panel" as="section">
      <div className="section-heading">
        <h2>Plan</h2>
        {plan ? (
          <Badge tone="success">Frozen</Badge>
        ) : proposal ? (
          <Badge tone="attention">Not frozen</Badge>
        ) : (
          <span className="muted">No plan recorded</span>
        )}
      </div>

      {plan ? (
        <div className="request-frozen-plan">
          <p className="muted">
            Core froze this exact plan identity. Approval and Campaign
            authorization must match it.
          </p>
          <dl className="request-facts">
            <div>
              <dt>Plan hash</dt>
              <dd>
                <code title={plan.plan_hash}>{plan.plan_hash}</code>
              </dd>
            </div>
            <div>
              <dt>Config hash</dt>
              <dd>
                <code title={plan.config_hash}>{plan.config_hash}</code>
              </dd>
            </div>
            <div>
              <dt>Delegation hash</dt>
              <dd>
                <code title={plan.delegation_hash}>{plan.delegation_hash}</code>
              </dd>
            </div>
            <div>
              <dt>Proposal</dt>
              <dd>
                <code>{shortId(plan.proposal_id)}</code>
              </dd>
            </div>
            <div>
              <dt>Frozen</dt>
              <dd>{formatTimestamp(plan.created_at)}</dd>
            </div>
          </dl>
        </div>
      ) : proposal ? (
        <p className="ui-callout ui-callout--attention">
          Core recorded a proposal but no frozen plan. Approval is unavailable
          until Core freezes the plan and exposes its plan hash.
        </p>
      ) : (
        <EmptyState
          title="No plan proposed"
          description="A frozen plan appears here after Core assesses the request. The model proposal never grants authority by itself."
        />
      )}

      {proposal && (
        <div className="request-proposal">
          <div className="section-heading request-proposal-heading">
            <h3>{plan ? "Frozen plan content" : "Proposed plan"}</h3>
            <span className="muted">
              {proposal.workUnits.length} work unit
              {proposal.workUnits.length === 1 ? "" : "s"}
            </span>
          </div>
          <dl className="request-facts">
            <div>
              <dt>Objective</dt>
              <dd>{proposal.objective}</dd>
            </div>
            <div>
              <dt>Class / risk</dt>
              <dd>
                {proposal.class} / {proposal.riskClass}
              </dd>
            </div>
            <div>
              <dt>Quality profile</dt>
              <dd>{proposal.qualityProfile}</dd>
            </div>
            <div>
              <dt>Route</dt>
              <dd>
                <code>
                  {proposal.route.provider}/{proposal.route.model}
                </code>{" "}
                ({proposal.route.mode})
              </dd>
            </div>
            <div>
              <dt>Budget</dt>
              <dd>
                {proposal.budget.maxAttempts} attempt
                {proposal.budget.maxAttempts === 1 ? "" : "s"} ·{" "}
                {proposal.budget.timeoutSecs}s timeout
              </dd>
            </div>
            <div>
              <dt>Execution bounds</dt>
              <dd>
                {proposal.maxSessions} session
                {proposal.maxSessions === 1 ? "" : "s"} · {proposal.maxParallel}{" "}
                parallel · {proposal.maxWallClockSecs}s wall clock ·{" "}
                {proposal.maxRepairCycles} repair cycle
                {proposal.maxRepairCycles === 1 ? "" : "s"}
              </dd>
            </div>
            <div>
              <dt>Checks</dt>
              <dd>
                {proposal.acceptanceCriteria.length} acceptance ·{" "}
                {proposal.negativeCases.length} negative
              </dd>
            </div>
          </dl>
          <dl className="request-facts request-facts--wide">
            <div>
              <dt>Assumptions</dt>
              <dd>{valuesText(proposal.assumptions)}</dd>
            </div>
            <div>
              <dt>Risks</dt>
              <dd>{valuesText(proposal.risks)}</dd>
            </div>
          </dl>

          <h4 className="request-workunits-heading">Work units</h4>
          {proposal.workUnits.length === 0 ? (
            <p className="muted">No work units recorded.</p>
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
                      <dt>Depends on</dt>
                      <dd>{valuesText(unit.dependsOn)}</dd>
                    </div>
                    <div>
                      <dt>Allowed paths</dt>
                      <dd>{valuesText(unit.allowedPaths)}</dd>
                    </div>
                    <div>
                      <dt>Capabilities</dt>
                      <dd>{valuesText(unit.capabilities)}</dd>
                    </div>
                    <div>
                      <dt>Checks</dt>
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
