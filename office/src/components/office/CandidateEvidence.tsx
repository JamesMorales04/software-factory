import type {
  Candidate,
  Gate,
  QualityReceipt,
  QualityRole,
  ReviewFinding,
} from "../../coreApi";
import { formatCostMicros, formatTimestamp, shortId } from "../../format";
import { useI18n } from "../../i18n";
import { StatusBadge } from "./StatusBadge";

export function ReceiptEvidenceTable({
  receipts,
}: {
  receipts: QualityReceipt[];
}) {
  const { t } = useI18n();
  const roles: QualityRole[] = ["verifier", "reviewer"];
  return (
    <div className="table-responsive">
      <table data-testid="candidate-receipts">
        <thead>
          <tr>
            <th>{t("evidence.receipts.role")}</th>
            <th>{t("evidence.receipts.result")}</th>
            <th>{t("evidence.receipts.evidence")}</th>
            <th>{t("evidence.receipts.principal")}</th>
            <th>{t("evidence.receipts.received")}</th>
          </tr>
        </thead>
        <tbody>
          {roles.map((role) => {
            const receipt = receipts.find((item) => item.role === role);
            if (!receipt) {
              return (
                <tr key={role}>
                  <th scope="row">{role}</th>
                  <td data-label={t("evidence.receipts.result")}>
                    <StatusBadge status="PENDING" />
                  </td>
                  <td
                    className="muted"
                    data-label={t("evidence.receipts.evidence")}
                  >
                    {t("evidence.receipts.noReceipt")}
                  </td>
                  <td
                    className="muted"
                    data-label={t("evidence.receipts.principal")}
                  >
                    —
                  </td>
                  <td
                    className="muted"
                    data-label={t("evidence.receipts.received")}
                  >
                    —
                  </td>
                </tr>
              );
            }
            return (
              <tr key={role}>
                <th scope="row">{role}</th>
                <td data-label={t("evidence.receipts.result")}>
                  <StatusBadge status={receipt.result} />
                </td>
                <td data-label={t("evidence.receipts.evidence")}>
                  <StatusBadge status={receipt.evidence} />
                </td>
                <td data-label={t("evidence.receipts.principal")}>
                  <code>{receipt.principal_id}</code>
                </td>
                <td
                  className="muted"
                  data-label={t("evidence.receipts.received")}
                >
                  {formatTimestamp(receipt.received_at)}
                  <div>
                    {t("evidence.receipts.receipt", {
                      id: shortId(receipt.receipt_id),
                    })}
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export function ReceiptModelProvenance({
  receipts,
}: {
  receipts: QualityReceipt[];
}) {
  const { t } = useI18n();
  const modelReceipts = receipts.filter(
    (receipt) => receipt.mechanism === "MODEL",
  );
  if (modelReceipts.length === 0) return null;
  return (
    <div data-testid="receipt-model-provenance">
      {modelReceipts.map((receipt) => (
        <p className="muted" key={receipt.receipt_id}>
          {t("evidence.provenance", {
            role: receipt.role,
            provider:
              receipt.model_provider || t("evidence.providerNotRecorded"),
            model: receipt.model_name || t("evidence.modelNotRecorded"),
            reservation: receipt.model_reservation_id
              ? shortId(receipt.model_reservation_id)
              : t("common.notRecorded"),
            request: receipt.model_request_sha256
              ? shortId(receipt.model_request_sha256, 12)
              : t("common.notRecorded"),
            usage: receipt.usage_known
              ? t("evidence.usage", {
                  input: receipt.input_tokens.toLocaleString(),
                  output: receipt.output_tokens.toLocaleString(),
                  total: receipt.total_tokens.toLocaleString(),
                  cost: formatCostMicros(receipt.cost_micros),
                })
              : t("evidence.usageUnmeasured"),
          })}
        </p>
      ))}
    </div>
  );
}

function FindingList({ findings }: { findings: ReviewFinding[] }) {
  if (findings.length === 0) return null;
  return (
    <ul className="findings">
      {findings.map((finding) => (
        <li key={finding.id}>
          <StatusBadge
            status={
              finding.severity === "blocking"
                ? "FAIL"
                : finding.severity === "warning"
                  ? "CHANGES_REQUESTED"
                  : "PENDING"
            }
            label={finding.severity}
          />{" "}
          <strong>{finding.category}</strong>: {finding.message}
          {finding.evidence_refs.length > 0 && (
            <span className="muted"> ({finding.evidence_refs.join(", ")})</span>
          )}
        </li>
      ))}
    </ul>
  );
}

export function ReviewFindings({ receipts }: { receipts: QualityReceipt[] }) {
  const withFindings = receipts.filter(
    (receipt) => receipt.findings.length > 0,
  );
  const { t } = useI18n();
  if (withFindings.length === 0) {
    return <p className="muted">{t("evidence.noFindings")}</p>;
  }
  return (
    <div data-testid="review-findings">
      {withFindings.map((receipt) => (
        <div key={receipt.receipt_id}>
          <p className="muted">
            {receipt.role} / {receipt.mechanism}
          </p>
          <FindingList findings={receipt.findings} />
        </div>
      ))}
    </div>
  );
}

export function QualityGatePanel({
  gate,
  candidateId,
}: {
  gate: Gate | null;
  candidateId: string;
}) {
  const { t } = useI18n();
  if (!gate) {
    return (
      <div className="ui-callout ui-callout--attention" role="status">
        <strong>PENDING</strong> —{" "}
        {t("evidence.gate.pending", { id: shortId(candidateId) })}
      </div>
    );
  }
  return (
    <div data-testid="quality-gate">
      <p>
        <StatusBadge status={gate.outcome} />{" "}
        <span className="muted">
          {t("evidence.gate.decided", {
            date: formatTimestamp(gate.decided_at),
          })}
        </span>
      </p>
      {gate.reasons.length === 0 ? (
        <p className="muted">{t("evidence.gate.noReasons")}</p>
      ) : (
        <ul className="findings">
          {gate.reasons.map((reason) => (
            <li key={reason}>{reason}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function CandidateIdentity({ candidate }: { candidate: Candidate }) {
  const { t } = useI18n();
  return (
    <dl className="detail">
      <dt>{t("evidence.identity.commit")}</dt>
      <dd>
        <code title={candidate.commit_sha}>
          {shortId(candidate.commit_sha, 12)}
        </code>
      </dd>
      <dt>{t("evidence.identity.tree")}</dt>
      <dd>
        <code title={candidate.tree_sha}>
          {shortId(candidate.tree_sha, 12)}
        </code>
      </dd>
      <dt>{t("evidence.identity.attempt")}</dt>
      <dd>
        <code>{shortId(candidate.attempt_id)}</code>
      </dd>
      <dt>{t("evidence.identity.implementer")}</dt>
      <dd>
        <code>{candidate.implementer_id}</code>
      </dd>
      <dt>{t("evidence.identity.recorded")}</dt>
      <dd>{formatTimestamp(candidate.created_at)}</dd>
    </dl>
  );
}
