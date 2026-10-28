import type { ParticipantSession, ProviderSession } from "../../coreApi";
import { formatCostMicros, formatTimestamp, shortId } from "../../format";
import { EmptyState } from "../ui/Surface";
import { StatusBadge } from "./StatusBadge";

const TERMINAL = new Set(["COMPLETED", "FAILED", "CLEANED", "ORPHANED"]);

export type PresentedSession = ProviderSession | ParticipantSession;

export function cleanupLabel(session: PresentedSession) {
  if (session.cleanup === "RECORDED" && session.cleanup_receipt) {
    return <code>{session.cleanup_receipt}</code>;
  }
  if (session.cleanup === "RECORDED") {
    return <span className="muted">recorded (receipt withheld)</span>;
  }
  if (session.cleanup === "MISSING") {
    return <span className="warning">no cleanup receipt</span>;
  }
  if (session.cleanup === "UNRECOGNIZED") {
    return <span className="warning">unrecognized cleanup receipt</span>;
  }
  if (session.cleanup === "PENDING") {
    return <span className="muted">not recorded yet</span>;
  }
  if (session.cleanup_receipt) {
    return <code>{session.cleanup_receipt}</code>;
  }
  if (TERMINAL.has(session.state) || session.state === "CANCEL_REQUESTED") {
    return <span className="warning">no cleanup receipt</span>;
  }
  return <span className="muted">not recorded yet</span>;
}

export function ParticipantSessionList({
  sessions,
  emptyTitle = "No provider sessions",
  emptyDescription = "No provider session is recorded for this scope.",
  showAttempt = true,
}: {
  sessions: PresentedSession[];
  emptyTitle?: string;
  emptyDescription?: string;
  showAttempt?: boolean;
}) {
  if (sessions.length === 0) {
    return <EmptyState title={emptyTitle} description={emptyDescription} />;
  }
  return (
    <table>
      <thead>
        <tr>
          <th>Session</th>
          {showAttempt && <th>Attempt</th>}
          <th>Worker / route</th>
          <th>State</th>
          <th>Usage</th>
          <th>Cleanup</th>
          <th>Updated</th>
        </tr>
      </thead>
      <tbody>
        {sessions.map((session) => (
          <tr key={session.session_record_id}>
            <td>
              <code>{shortId(session.session_record_id)}</code>
              <div className="muted">generation {session.generation}</div>
            </td>
            {showAttempt && (
              <td>
                <code>{shortId(session.attempt_id)}</code>
              </td>
            )}
            <td>
              {session.provider}/{session.model}
              <div className="muted">
                {session.worker_id} · {session.provider_mode}
              </div>
            </td>
            <td>
              <StatusBadge status={session.state} />
            </td>
            <td>
              {session.usage_known ? (
                <>
                  {session.input_tokens.toLocaleString()} in /{" "}
                  {session.output_tokens.toLocaleString()} out
                  <div className="muted">
                    {session.total_tokens.toLocaleString()} tokens ·{" "}
                    {formatCostMicros(session.cost_micros)}
                  </div>
                </>
              ) : (
                <StatusBadge status="UNMEASURED" label="UNMEASURED" />
              )}
              {session.error && <div className="error">{session.error}</div>}
            </td>
            <td>{cleanupLabel(session)}</td>
            <td className="muted">{formatTimestamp(session.updated_at)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
