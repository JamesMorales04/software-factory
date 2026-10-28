import type { ParticipantSession, ProviderSession } from "../../coreApi";
import { shortId } from "../../format";
import { type Translate, useI18n } from "../../i18n";
import { EmptyState } from "../ui/Surface";
import { StatusBadge } from "./StatusBadge";

const TERMINAL = new Set(["COMPLETED", "FAILED", "CLEANED", "ORPHANED"]);

export type PresentedSession = ProviderSession | ParticipantSession;

export function cleanupLabel(session: PresentedSession, t: Translate) {
  if (session.cleanup === "RECORDED" && session.cleanup_receipt) {
    return <code>{session.cleanup_receipt}</code>;
  }
  if (session.cleanup === "RECORDED") {
    return (
      <span className="muted">{t("sessions.cleanup.recordedWithheld")}</span>
    );
  }
  if (session.cleanup === "MISSING") {
    return <span className="warning">{t("sessions.cleanup.missing")}</span>;
  }
  if (session.cleanup === "UNRECOGNIZED") {
    return (
      <span className="warning">{t("sessions.cleanup.unrecognized")}</span>
    );
  }
  if (session.cleanup === "PENDING") {
    return <span className="muted">{t("sessions.cleanup.notRecorded")}</span>;
  }
  if (session.cleanup_receipt) {
    return <code>{session.cleanup_receipt}</code>;
  }
  if (TERMINAL.has(session.state) || session.state === "CANCEL_REQUESTED") {
    return <span className="warning">{t("sessions.cleanup.missing")}</span>;
  }
  return <span className="muted">{t("sessions.cleanup.notRecorded")}</span>;
}

export function ParticipantSessionList({
  sessions,
  emptyTitle,
  emptyDescription,
  showAttempt = true,
}: {
  sessions: PresentedSession[];
  emptyTitle?: string;
  emptyDescription?: string;
  showAttempt?: boolean;
}) {
  const { t, formatDate, formatNumber, formatMicros } = useI18n();
  if (sessions.length === 0) {
    return (
      <EmptyState
        title={emptyTitle ?? t("sessions.emptyTitle")}
        description={emptyDescription ?? t("sessions.emptyDescription")}
      />
    );
  }
  return (
    <section
      className="table-responsive"
      aria-label={t("sessions.table.label")}
    >
      <table>
        <thead>
          <tr>
            <th>{t("sessions.table.session")}</th>
            {showAttempt && <th>{t("sessions.table.attempt")}</th>}
            <th>{t("sessions.table.route")}</th>
            <th>{t("sessions.table.state")}</th>
            <th>{t("sessions.table.usage")}</th>
            <th>{t("sessions.table.cleanup")}</th>
            <th>{t("sessions.table.updated")}</th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((session) => (
            <tr key={session.session_record_id}>
              <td data-label={t("sessions.table.session")}>
                <code>{shortId(session.session_record_id)}</code>
                <div className="muted">
                  {t("sessions.generation", {
                    generation: session.generation,
                  })}
                </div>
              </td>
              {showAttempt && (
                <td data-label={t("sessions.table.attempt")}>
                  <code>{shortId(session.attempt_id)}</code>
                </td>
              )}
              <td data-label={t("sessions.table.route")}>
                {session.provider}/{session.model}
                <div className="muted">
                  {session.worker_id} · {session.provider_mode}
                </div>
              </td>
              <td data-label={t("sessions.table.state")}>
                <StatusBadge status={session.state} />
              </td>
              <td data-label={t("sessions.table.usage")}>
                {session.usage_known ? (
                  <>
                    {t("sessions.usage.inputOutput", {
                      input: formatNumber(session.input_tokens),
                      output: formatNumber(session.output_tokens),
                    })}
                    <div className="muted">
                      {t("sessions.usage.total", {
                        tokens: formatNumber(session.total_tokens),
                        cost: formatMicros(session.cost_micros),
                      })}
                    </div>
                  </>
                ) : (
                  <StatusBadge status="UNMEASURED" label="UNMEASURED" />
                )}
                {session.error && <div className="error">{session.error}</div>}
              </td>
              <td data-label={t("sessions.table.cleanup")}>
                {cleanupLabel(session, t)}
              </td>
              <td className="muted" data-label={t("sessions.table.updated")}>
                {formatDate(session.updated_at)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
