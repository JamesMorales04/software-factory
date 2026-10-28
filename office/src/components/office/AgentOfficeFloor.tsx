import type { ParticipantRecord, ParticipantRole } from "../../coreApi";
import { shortId } from "../../format";
import { useI18n } from "../../i18n";
import { cn } from "../ui/cn";
import { EmptyState } from "../ui/Surface";
import { StatusBadge } from "./StatusBadge";

interface Zone {
  id: string;
  labelKey:
    | "agentOffice.zone.planning"
    | "agentOffice.zone.implementation"
    | "agentOffice.zone.verification"
    | "agentOffice.zone.review"
    | "agentOffice.zone.operations";
  role: ParticipantRole;
  descriptionKey:
    | "agentOffice.zone.planningDescription"
    | "agentOffice.zone.implementationDescription"
    | "agentOffice.zone.verificationDescription"
    | "agentOffice.zone.reviewDescription"
    | "agentOffice.zone.operationsDescription";
}

const ZONES: Zone[] = [
  {
    id: "planning",
    labelKey: "agentOffice.zone.planning",
    role: "LEAD",
    descriptionKey: "agentOffice.zone.planningDescription",
  },
  {
    id: "implementation",
    labelKey: "agentOffice.zone.implementation",
    role: "IMPLEMENTER",
    descriptionKey: "agentOffice.zone.implementationDescription",
  },
  {
    id: "verification",
    labelKey: "agentOffice.zone.verification",
    role: "VERIFIER",
    descriptionKey: "agentOffice.zone.verificationDescription",
  },
  {
    id: "review",
    labelKey: "agentOffice.zone.review",
    role: "REVIEWER",
    descriptionKey: "agentOffice.zone.reviewDescription",
  },
  {
    id: "operations",
    labelKey: "agentOffice.zone.operations",
    role: "WORKER",
    descriptionKey: "agentOffice.zone.operationsDescription",
  },
];

function roleLabel(
  role: ParticipantRole,
  t: ReturnType<typeof useI18n>["t"],
): string {
  switch (role) {
    case "IMPLEMENTER":
      return t("agentOffice.role.IMPLEMENTER");
    case "LEAD":
      return t("agentOffice.role.LEAD");
    case "VERIFIER":
      return t("agentOffice.role.VERIFIER");
    case "REVIEWER":
      return t("agentOffice.role.REVIEWER");
    case "WORKER":
      return t("agentOffice.role.WORKER");
  }
}

function routeLabel(
  participant: ParticipantRecord,
  t: ReturnType<typeof useI18n>["t"],
): string {
  const route = participant.requested_route ?? participant.observed_route;
  if (!route) return t("agentOffice.route.notExposed");
  const source = participant.requested_route
    ? t("agentOffice.route.requested")
    : t("agentOffice.route.observed");
  return `${route.provider}/${route.model} (${source})`;
}

function assignmentLabel(
  participant: ParticipantRecord,
  t: ReturnType<typeof useI18n>["t"],
): string {
  if (participant.work_unit_id && participant.campaign_id) {
    return t("agentOffice.assignment.unitCampaign", {
      unit: participant.work_unit_id,
      campaign: shortId(participant.campaign_id),
    });
  }
  if (participant.campaign_id) {
    return t("agentOffice.assignment.campaign", {
      id: shortId(participant.campaign_id),
    });
  }
  if (participant.request_id) {
    return t("agentOffice.assignment.leadRequest", {
      id: shortId(participant.request_id),
    });
  }
  return t("agentOffice.assignment.none");
}

function observationLabel(participant: ParticipantRecord): string {
  if (participant.observation === "STALE") return "STALE";
  if (participant.observation === "CURRENT") return "CURRENT";
  if (participant.observation === "FINAL") return "FINAL";
  if (participant.observation === "NOT_EXPOSED") return "NOT EXPOSED";
  return "UNKNOWN";
}

export function ParticipantCard({
  participant,
  selected,
  onSelect,
}: {
  participant: ParticipantRecord;
  selected: boolean;
  onSelect: (participantId: string) => void;
}) {
  const { t, formatDate } = useI18n();
  return (
    <button
      type="button"
      className={cn(
        "agent-card",
        selected && "is-selected",
        `agent-observation-${participant.observation.toLowerCase()}`,
      )}
      aria-pressed={selected}
      aria-label={t("agentOffice.card.inspect", {
        role: roleLabel(participant.role, t),
        id: participant.participant_id,
        status: participant.status,
        observation: observationLabel(participant),
      })}
      onClick={() => onSelect(participant.participant_id)}
      data-testid={`agent-card-${participant.participant_id}`}
    >
      <span className="agent-card-top">
        <span className="agent-card-role">
          {roleLabel(participant.role, t)}
        </span>
        <StatusBadge status={participant.status} />
      </span>
      <span className="agent-card-id">
        <code>{shortId(participant.identity_id, 18)}</code>
      </span>
      <span className="agent-card-route">{routeLabel(participant, t)}</span>
      <span className="agent-card-assignment">
        {assignmentLabel(participant, t)}
      </span>
      <span className="agent-card-footer">
        <StatusBadge
          status={participant.observation}
          label={observationLabel(participant)}
        />
        <time dateTime={participant.last_observed_at}>
          {formatDate(participant.last_observed_at)}
        </time>
      </span>
      {participant.failure_reason && (
        <span className="agent-card-error">{participant.failure_reason}</span>
      )}
    </button>
  );
}

export function AgentOfficeFloor({
  participants,
  selectedId,
  onSelect,
}: {
  participants: ParticipantRecord[];
  selectedId: string | null;
  onSelect: (participantId: string) => void;
}) {
  const { t, formatNumber } = useI18n();
  return (
    <div className="agent-floor" data-testid="agent-floor">
      {ZONES.map((zone) => {
        const members = participants.filter(
          (participant) => participant.role === zone.role,
        );
        return (
          <section
            key={zone.id}
            className={cn("agent-zone", `agent-zone-${zone.id}`)}
            aria-labelledby={`agent-zone-${zone.id}`}
          >
            <header className="agent-zone-header">
              <div>
                <h3 id={`agent-zone-${zone.id}`}>{t(zone.labelKey)}</h3>
                <p className="muted">{t(zone.descriptionKey)}</p>
              </div>
              <span className="agent-zone-count">
                {t("agentOffice.zone.recorded", {
                  count: formatNumber(members.length),
                })}
              </span>
            </header>
            {members.length === 0 ? (
              <EmptyState
                title={t("agentOffice.zone.emptyTitle", {
                  role: roleLabel(zone.role, t).toLowerCase(),
                })}
                description={t("agentOffice.zone.emptyDescription")}
              />
            ) : (
              <div className="agent-zone-grid">
                {members.map((participant) => (
                  <ParticipantCard
                    key={participant.participant_id}
                    participant={participant}
                    selected={participant.participant_id === selectedId}
                    onSelect={onSelect}
                  />
                ))}
              </div>
            )}
          </section>
        );
      })}
    </div>
  );
}

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
