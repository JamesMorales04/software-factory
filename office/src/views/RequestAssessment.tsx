import type { ReactNode } from "react";
import { EmptyState, Panel } from "../components/ui/Surface";
import type { LeadAssessmentView } from "../coreApi";

function ValueList({ values }: { values: string[] }) {
  const unique = [...new Set(values)];
  if (unique.length === 0) {
    return <span className="muted">None recorded</span>;
  }
  return (
    <ul className="request-value-list">
      {unique.map((value) => (
        <li key={value}>{value}</li>
      ))}
    </ul>
  );
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function effortText(assessment: LeadAssessmentView): ReactNode {
  const effort = assessment.effortEstimate;
  if (effort.status === "estimated") {
    return `${effort.minPersonHours}–${effort.maxPersonHours} ${effort.unit}`;
  }
  return (
    <>
      {effort.status.replaceAll("_", " ")}
      {effort.unknownReason ? `: ${effort.unknownReason}` : ""}
    </>
  );
}

export function RequestAssessment({
  assessment,
}: {
  assessment: LeadAssessmentView | null;
}) {
  return (
    <Panel className="request-panel" as="section">
      <div className="section-heading">
        <h2>Assessment</h2>
        <span className="muted">Core-recorded semantic assessment</span>
      </div>
      {assessment === null ? (
        <EmptyState
          title="No assessment recorded"
          description="Core records an assessment after the Lead route runs. It is advisory and does not authorize a Campaign."
        />
      ) : (
        <>
          <dl className="request-facts">
            <Fact label="Intent">{assessment.intent}</Fact>
            <Fact label="Complexity">{assessment.complexity}</Fact>
            <Fact label="Effort">{effortText(assessment)}</Fact>
            <Fact label="Suggested next action">
              {assessment.suggestedNextAction}
            </Fact>
          </dl>
          <dl className="request-facts request-facts--wide">
            <Fact label="Change impact">
              <ValueList values={assessment.changeImpact} />
            </Fact>
            <Fact label="Validation needs">
              <ValueList values={assessment.validationNeeds} />
            </Fact>
            <Fact label="Knowledge gaps">
              <ValueList values={assessment.knowledgeGaps} />
            </Fact>
            <Fact label="Assumptions">
              <ValueList values={assessment.assumptions} />
            </Fact>
            <Fact label="Uncertainty">
              <ValueList values={assessment.uncertainty} />
            </Fact>
            <Fact label="Evidence references">
              <ValueList values={assessment.evidenceRefs} />
            </Fact>
          </dl>
          <p className="muted request-footnote">
            Assessment is model-assisted judgment. Core policy, the frozen plan
            and the authorization remain authoritative.
          </p>
        </>
      )}
    </Panel>
  );
}
