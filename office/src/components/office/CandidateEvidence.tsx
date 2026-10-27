import type {
  Candidate,
  Gate,
  QualityReceipt,
  QualityRole,
  ReviewFinding,
} from "../../coreApi";
import { formatCostMicros, formatTimestamp, shortId } from "../../format";
import { StatusBadge } from "./StatusBadge";

export function ReceiptEvidenceTable({
  receipts,
}: {
  receipts: QualityReceipt[];
}) {
  const roles: QualityRole[] = ["verifier", "reviewer"];
  return (
    <table data-testid="candidate-receipts">
      <thead>
        <tr>
          <th>Role</th>
          <th>Result</th>
          <th>Evidence</th>
          <th>Principal</th>
          <th>Received</th>
        </tr>
      </thead>
      <tbody>
        {roles.map((role) => {
          const receipt = receipts.find((item) => item.role === role);
          if (!receipt) {
            return (
              <tr key={role}>
                <th scope="row">{role}</th>
                <td>
                  <StatusBadge status="PENDING" />
                </td>
                <td className="muted">no receipt recorded</td>
                <td className="muted">—</td>
                <td className="muted">—</td>
              </tr>
            );
          }
          return (
            <tr key={role}>
              <th scope="row">{role}</th>
              <td>
                <StatusBadge status={receipt.result} />
              </td>
              <td>
                <StatusBadge status={receipt.evidence} />
              </td>
              <td>
                <code>{receipt.principal_id}</code>
              </td>
              <td className="muted">
                {formatTimestamp(receipt.received_at)}
                <div>
                  receipt <code>{shortId(receipt.receipt_id)}</code>
                </div>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

export function ReceiptModelProvenance({
  receipts,
}: {
  receipts: QualityReceipt[];
}) {
  const modelReceipts = receipts.filter(
    (receipt) => receipt.mechanism === "MODEL",
  );
  if (modelReceipts.length === 0) return null;
  return (
    <div data-testid="receipt-model-provenance">
      {modelReceipts.map((receipt) => (
        <p className="muted" key={receipt.receipt_id}>
          {receipt.role} model:{" "}
          {receipt.model_provider || "provider not recorded"}/
          {receipt.model_name || "model not recorded"} · reservation{" "}
          <code>
            {receipt.model_reservation_id
              ? shortId(receipt.model_reservation_id)
              : "not recorded"}
          </code>{" "}
          · request{" "}
          <code>
            {receipt.model_request_sha256
              ? shortId(receipt.model_request_sha256, 12)
              : "not recorded"}
          </code>{" "}
          ·{" "}
          {receipt.usage_known
            ? `${receipt.input_tokens.toLocaleString()} in / ${receipt.output_tokens.toLocaleString()} out / ${receipt.total_tokens.toLocaleString()} total / ${formatCostMicros(receipt.cost_micros)}`
            : "usage UNMEASURED"}
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
  if (withFindings.length === 0) {
    return <p className="muted">No findings recorded.</p>;
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
  if (!gate) {
    return (
      <div className="ui-callout ui-callout--attention" role="status">
        <strong>PENDING</strong> — no Core gate decision is recorded for
        candidate <code>{shortId(candidateId)}</code>. Missing receipts or
        non-passing evidence are not approval.
      </div>
    );
  }
  return (
    <div data-testid="quality-gate">
      <p>
        <StatusBadge status={gate.outcome} />{" "}
        <span className="muted">
          decided {formatTimestamp(gate.decided_at)}
        </span>
      </p>
      {gate.reasons.length === 0 ? (
        <p className="muted">No reasons recorded.</p>
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
  return (
    <dl className="detail">
      <dt>Commit</dt>
      <dd>
        <code title={candidate.commit_sha}>
          {shortId(candidate.commit_sha, 12)}
        </code>
      </dd>
      <dt>Tree</dt>
      <dd>
        <code title={candidate.tree_sha}>
          {shortId(candidate.tree_sha, 12)}
        </code>
      </dd>
      <dt>Attempt</dt>
      <dd>
        <code>{shortId(candidate.attempt_id)}</code>
      </dd>
      <dt>Implementer</dt>
      <dd>
        <code>{candidate.implementer_id}</code>
      </dd>
      <dt>Recorded</dt>
      <dd>{formatTimestamp(candidate.created_at)}</dd>
    </dl>
  );
}
