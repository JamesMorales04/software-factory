import { useEffect, useState } from "react";
import {
  CoreApiError,
  coreApi,
  type ModelCallRecord,
  type ObservabilityStatus,
  type ProviderSession,
} from "../coreApi";
import {
  formatCostMicros,
  formatTimestamp,
  sessionStateClass,
  shortId,
} from "../format";

export function Runtime() {
  const [sessions, setSessions] = useState<ProviderSession[]>([]);
  const [modelCalls, setModelCalls] = useState<ModelCallRecord[]>([]);
  const [observability, setObservability] =
    useState<ObservabilityStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [next, calls, telemetry] = await Promise.all([
          coreApi.listProviderSessions(),
          coreApi.listModelCalls(100),
          coreApi.getObservability(),
        ]);
        if (active) {
          setSessions(next);
          setModelCalls(calls);
          setObservability(telemetry);
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
    ["INTENT", "ACTIVE", "CANCEL_REQUESTED", "ORPHANED"].includes(
      session.state,
    ),
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
      <h3>Core-owned model calls</h3>
      <p className="muted">
        Lead, semantic Reviewer and CAP-014 calls reserve a bounded token/cost
        envelope in Core before the upstream request. AMBIGUOUS or UNKNOWN calls
        consume budget until an operator resolves them and are never retried
        automatically.
      </p>
      {modelCalls.length === 0 ? (
        <p className="muted">No model-call reservations recorded.</p>
      ) : (
        <table>
          <thead>
            <tr>
              <th>Role</th>
              <th>Scope</th>
              <th>Route</th>
              <th>State</th>
              <th>Tokens / cost</th>
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            {modelCalls.map((call) => (
              <tr key={call.reservation_id}>
                <td>{call.role}</td>
                <td>
                  <code>{shortId(call.scope_id)}</code>
                  <div className="muted">{call.purpose}</div>
                </td>
                <td>
                  {call.provider}/{call.model}
                </td>
                <td>
                  <span className={sessionStateClass(call.state)}>
                    {call.state}
                  </span>
                  {call.error && <div className="error">{call.error}</div>}
                </td>
                <td>
                  {call.usage_known
                    ? `${call.total_tokens} / ${formatCostMicros(call.cost_micros)}`
                    : "UNMEASURED"}
                </td>
                <td className="muted">{formatTimestamp(call.updated_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <h3>Telemetry export</h3>
      {observability === null ? (
        <p className="muted">No observability status available.</p>
      ) : (
        <section className="recovery-summary">
          <p>
            Export{" "}
            <strong>{observability.enabled ? "ENABLED" : "DISABLED"}</strong> at
            cursor <strong>{observability.cursor}</strong>;{" "}
            <strong>{observability.exported}</strong> exported,{" "}
            <strong>{observability.pending}</strong> pending,{" "}
            <strong>{observability.dropped}</strong> dropped hints,{" "}
            <strong>{observability.failed_attempts}</strong> failed attempts.
          </p>
          {observability.last_error && (
            <p className="error">{observability.last_error}</p>
          )}
          <p className="muted">
            Exporter failure is telemetry degradation, not authoritative Factory
            state.
          </p>
        </section>
      )}
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
                  <span className={sessionStateClass(session.state)}>
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
