import type { ReactNode } from "react";
import { EmptyState, Panel } from "../components/ui/Surface";
import type { LeadAssessmentView } from "../coreApi";
import { useI18n } from "../i18n";

function ValueList({ values }: { values: string[] }) {
  const { t } = useI18n();
  const unique = [...new Set(values)];
  if (unique.length === 0) {
    return <span className="muted">{t("request.values.noneRecorded")}</span>;
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
  const { t } = useI18n();
  return (
    <Panel className="request-panel" as="section">
      <div className="section-heading">
        <h2>{t("request.assessment.title")}</h2>
        <span className="muted">{t("request.assessment.hint")}</span>
      </div>
      {assessment === null ? (
        <EmptyState
          title={t("request.assessment.emptyTitle")}
          description={t("request.assessment.emptyDescription")}
        />
      ) : (
        <>
          <dl className="request-facts">
            <Fact label={t("request.assessment.intent")}>
              {assessment.intent}
            </Fact>
            <Fact label={t("request.assessment.complexity")}>
              {assessment.complexity}
            </Fact>
            <Fact label={t("request.assessment.effort")}>
              {effortText(assessment)}
            </Fact>
            <Fact label={t("request.assessment.suggestedNextAction")}>
              {assessment.suggestedNextAction}
            </Fact>
          </dl>
          <dl className="request-facts request-facts--wide">
            <Fact label={t("request.assessment.changeImpact")}>
              <ValueList values={assessment.changeImpact} />
            </Fact>
            <Fact label={t("request.assessment.validationNeeds")}>
              <ValueList values={assessment.validationNeeds} />
            </Fact>
            <Fact label={t("request.assessment.knowledgeGaps")}>
              <ValueList values={assessment.knowledgeGaps} />
            </Fact>
            <Fact label={t("request.assessment.assumptions")}>
              <ValueList values={assessment.assumptions} />
            </Fact>
            <Fact label={t("request.assessment.uncertainty")}>
              <ValueList values={assessment.uncertainty} />
            </Fact>
            <Fact label={t("request.assessment.evidenceRefs")}>
              <ValueList values={assessment.evidenceRefs} />
            </Fact>
          </dl>
          <p className="muted request-footnote">
            {t("request.assessment.footnote")}
          </p>
        </>
      )}
    </Panel>
  );
}
