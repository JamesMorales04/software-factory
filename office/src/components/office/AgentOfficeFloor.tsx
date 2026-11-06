import type { ParticipantRecord } from "../../coreApi";
import { shortId } from "../../format";
import { useI18n } from "../../i18n";
import { cn } from "../ui/cn";
import { EmptyState } from "../ui/Surface";
import {
  assignmentLabel,
  observationLabel,
  roleLabel,
  routeLabel,
} from "./participantLabels";
import { StatusBadge } from "./StatusBadge";

export function AgentOfficeTable({
  participants,
  selectedId,
  onSelect,
}: {
  participants: ParticipantRecord[];
  selectedId: string | null;
  onSelect: (participantId: string) => void;
}) {
  const { t, formatNumber, formatDate } = useI18n();
  if (participants.length === 0) {
    return (
      <EmptyState
        title={t("agentOffice.emptyTableTitle")}
        description={t("agentOffice.emptyTableDescription")}
      />
    );
  }
  return (
    <div className="agent-table-wrap table-responsive">
      <table
        data-testid="agent-table"
        aria-label={t("agentOffice.table.label")}
      >
        <thead>
          <tr>
            <th>{t("agentOffice.table.roleParticipant")}</th>
            <th>{t("agentOffice.table.status")}</th>
            <th>{t("agentOffice.table.observation")}</th>
            <th>{t("agentOffice.table.route")}</th>
            <th>{t("agentOffice.table.assignment")}</th>
            <th>{t("agentOffice.table.session")}</th>
            <th>{t("agentOffice.table.lastObserved")}</th>
          </tr>
        </thead>
        <tbody>
          {participants.map((participant) => (
            <tr
              key={participant.participant_id}
              className={cn(
                participant.participant_id === selectedId && "is-selected",
              )}
            >
              <th
                scope="row"
                data-label={t("agentOffice.table.roleParticipant")}
              >
                <button
                  type="button"
                  className="link-button"
                  aria-pressed={participant.participant_id === selectedId}
                  onClick={() => onSelect(participant.participant_id)}
                >
                  {roleLabel(participant.role, t)}{" "}
                  <code>{shortId(participant.identity_id, 18)}</code>
                </button>
                <div className="muted">
                  {participant.identity_kind.replaceAll("_", " ")}
                </div>
              </th>
              <td data-label={t("agentOffice.table.status")}>
                <StatusBadge status={participant.status} />
              </td>
              <td data-label={t("agentOffice.table.observation")}>
                <StatusBadge
                  status={participant.observation}
                  label={observationLabel(participant)}
                />
                <div className="muted">{participant.lease_state}</div>
              </td>
              <td data-label={t("agentOffice.table.route")}>
                {routeLabel(participant, t)}
              </td>
              <td data-label={t("agentOffice.table.assignment")}>
                {assignmentLabel(participant, t)}
              </td>
              <td data-label={t("agentOffice.table.session")}>
                {participant.session ? (
                  <>
                    <code>
                      {shortId(participant.session.session_record_id, 14)}
                    </code>
                    <div className="muted">
                      {t("agentOffice.table.generation", {
                        generation: formatNumber(
                          participant.session.generation,
                        ),
                      })}
                    </div>
                  </>
                ) : (
                  <span className="muted">
                    {t("agentOffice.table.noSession")}
                  </span>
                )}
              </td>
              <td
                className="muted"
                data-label={t("agentOffice.table.lastObserved")}
              >
                <time dateTime={participant.last_observed_at}>
                  {formatDate(participant.last_observed_at)}
                </time>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function AgentOfficeTimeline({
  participants,
  selectedId,
  onSelect,
}: {
  participants: ParticipantRecord[];
  selectedId: string | null;
  onSelect: (participantId: string) => void;
}) {
  const { t, formatDate } = useI18n();
  if (participants.length === 0) {
    return (
      <EmptyState
        title={t("agentOffice.emptyTimelineTitle")}
        description={t("agentOffice.emptyTimelineDescription")}
      />
    );
  }
  const ordered = [...participants].sort((left, right) =>
    right.last_observed_at.localeCompare(left.last_observed_at),
  );
  return (
    <div className="agent-timeline-wrap">
      <p className="muted">{t("agentOffice.timeline.note")}</p>
      <ol className="agent-timeline" data-testid="agent-timeline">
        {ordered.map((participant) => (
          <li key={participant.participant_id}>
            <time dateTime={participant.last_observed_at}>
              {formatDate(participant.last_observed_at)}
            </time>
            <button
              type="button"
              className={cn(
                "agent-timeline-entry",
                participant.participant_id === selectedId && "is-selected",
              )}
              aria-pressed={participant.participant_id === selectedId}
              onClick={() => onSelect(participant.participant_id)}
            >
              <span>
                <strong>{roleLabel(participant.role, t)}</strong>{" "}
                <code>{shortId(participant.identity_id, 18)}</code>
              </span>
              <span className="agent-timeline-meta">
                <StatusBadge status={participant.status} />
                <StatusBadge
                  status={participant.observation}
                  label={observationLabel(participant)}
                />
                <span className="muted">{assignmentLabel(participant, t)}</span>
              </span>
              <span className="muted">
                {t("agentOffice.timeline.recorded", {
                  date: formatDate(participant.recorded_at),
                  source: participant.status_source,
                })}
              </span>
            </button>
          </li>
        ))}
      </ol>
    </div>
  );
}
