import { useState } from "react";
import { type CheapOpsReport, CoreApiError, coreApi } from "../../coreApi";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { Panel } from "../ui/Surface";

function errorText(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

export function CheapOpsPanel({ candidateId }: { candidateId: string }) {
  const { t } = useI18n();
  const [report, setReport] = useState<CheapOpsReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function interpret() {
    setBusy(true);
    setError(null);
    try {
      const result = await coreApi.getCandidateInterpretation(candidateId);
      setReport(result.report);
    } catch (cause) {
      setError(errorText(cause, t("cheapOps.error")));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel>
      <div className="section-heading">
        <h2>{t("cheapOps.title")}</h2>
        <span className="muted">{t("cheapOps.authority")}</span>
      </div>
      <p className="muted">{t("cheapOps.description")}</p>
      {error && (
        <p className="ui-callout ui-callout--danger" role="alert">
          {error}
        </p>
      )}
      {report ? (
        <>
          <p>{report.summary}</p>
          <dl className="campaign-facts">
            <div>
              <dt>{t("cheapOps.authorityLabel")}</dt>
              <dd>{report.authority}</dd>
            </div>
            <div>
              <dt>{t("cheapOps.reviewers")}</dt>
              <dd>{report.reviewer_count}</dd>
            </div>
            <div>
              <dt>{t("cheapOps.verifier")}</dt>
              <dd>
                {report.verifier_present
                  ? t("cheapOps.recorded")
                  : t("cheapOps.missing")}
              </dd>
            </div>
            <div>
              <dt>{t("cheapOps.gate")}</dt>
              <dd>{report.gate_outcome ?? t("cheapOps.pending")}</dd>
            </div>
          </dl>
          {report.notes.length > 0 && (
            <ul>
              {report.notes.map((note) => (
                <li key={note}>{note}</li>
              ))}
            </ul>
          )}
        </>
      ) : (
        <Button
          variant="secondary"
          size="sm"
          busy={busy}
          disabled={busy}
          onClick={() => void interpret()}
        >
          {busy ? t("cheapOps.working") : t("cheapOps.action")}
        </Button>
      )}
    </Panel>
  );
}
