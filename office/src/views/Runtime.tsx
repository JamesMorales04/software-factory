import { useEffect, useState } from "react";
import {
  CoreApiError,
  coreApi,
  type ProviderSession,
} from "../coreApi";
import { formatTimestamp, shortId } from "../format";

function cleanupClass(state: string) {
  if (state === "CLEANED" || state === "COMPLETED") return "pill recovery-confirmed";
  if (state === "ORPHANED" || state === "FAILED") return "pill recovery-blocked";
  return "pill";
}

export function Runtime() {
  const [sessions, setSessions] = useState<ProviderSession[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const next = await coreApi.listProviderSessions();
        if (active) {
          setSessions(next);
          setError(null);
        }
      } catch (cause) {
        if (active) {
          setError(
            cause instanceof CoreApiError
              ? cause.message
              : "Failed to load provider sessions.",
          );
        }
      }
    }
    void load();
    const timer = setInterval(() => void load(), 3000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  const unresolved = sessions.filter((session) =>
    ["INTENT", "ACTIVE", "CANCEL_REQUESTED", "ORPHANED"].includes(session.state),
  ).length;

  return (
    <div>
      <h2>Runtime</h2>
      <p className="muted">
        Durable provider-session and cleanup projections. A cleanup receipt is
        evidence recorded by the Worker; it is not an isolation certificate.
      </p>
      {error && <p className="error">{error}</p>}
      <section className="recovery-summary">
        <p>
          <strong>{unresolved}</strong> unresolved session
          {unresolved === 1 ? "" : "s"} across{" "}
          <strong>{sessions.length}</strong> recorded session
          {sessions.length === 1 ? "" : "s"}.
        </p>
      </section>
      {sessions.length === 0 ? (
        <p className="muted">No provider sessions recorded.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Session</th>
              <th>Attempt</th>
              <th>Route</th>
              <th>State</th>
              <th>Tokens / cost</th>
              <th>Cleanup</th>
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            {sessions.map((session) => (
              <tr key={session.session_record_id}>
                <td>
                  <code>{shortId(session.session_record_id)}</code>
                </td>
                <td>
                  <code>{shortId(session.attempt_id)}</code>
                </td>
                <td>
                  {session.provider}/{session.model}
                  <br />
                  <span className="muted">{session.provider_mode}</span>
                </td>
                <td>
                  <span className={cleanupClass(session.state)}>
                    {session.state}
                  </span>
                </td>
                <td>
                  {session.usage_known
                    ? `${session.total_tokens} / ${session.cost_micros}µ`
                    : "UNMEASURED"}
                </td>
                <td>
                  {session.cleanup_receipt || "—"}
                  {session.error && (
                    <span className="error"> {session.error}</span>
                  )}
                </td>
                <td className="muted">{formatTimestamp(session.updated_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
