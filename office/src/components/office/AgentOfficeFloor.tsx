import type { ParticipantRecord, ParticipantRole } from "../../coreApi";
import { formatTimestamp, shortId } from "../../format";
import { cn } from "../ui/cn";
import { EmptyState } from "../ui/Surface";
import { StatusBadge } from "./StatusBadge";

interface Zone {
  id: string;
  label: string;
  role: ParticipantRole;
  description: string;
}

const ZONES: Zone[] = [
  {
    id: "planning",
    label: "Planning",
    role: "LEAD",
    description:
      "Core-recorded planning and assessment operations. A model call is not a continuously running agent.",
  },
  {
    id: "implementation",
    label: "Implementation",
    role: "IMPLEMENTER",
    description:
      "Observed provider sessions bound to an Attempt and lease generation.",
  },
  {
    id: "verification",
    label: "Verification",
    role: "VERIFIER",
    description: "Independent verification receipts recorded by Core.",
  },
  {
    id: "review",
    label: "Review",
    role: "REVIEWER",
    description:
      "Independent review receipts recorded by Core, including mechanism and provenance.",
  },
  {
    id: "operations",
    label: "Operations",
    role: "WORKER",
    description:
      "Worker registration identity. Core does not expose heartbeat liveness in this projection.",
  },
];

const ROLE_LABEL: Record<ParticipantRole, string> = {
  IMPLEMENTER: "Implementer",
  LEAD: "Lead",
  VERIFIER: "Verifier",
  REVIEWER: "Reviewer",
  WORKER: "Worker",
};

function routeLabel(participant: ParticipantRecord): string {
  const route = participant.requested_route ?? participant.observed_route;
  if (!route) return "Route not exposed";
  const source =
    participant.requested_route &&
    participant.observed_route &&
    participant.requested_route.provider !== participant.observed_route.provider
      ? "requested"
      : participant.requested_route
        ? "requested"
        : "observed";
  return `${route.provider}/${route.model} (${source})`;
}

function assignmentLabel(participant: ParticipantRecord): string {
  if (participant.work_unit_id && participant.campaign_id) {
    return `Unit ${participant.work_unit_id} · Campaign ${shortId(participant.campaign_id)}`;
  }
  if (participant.campaign_id) {
    return `Campaign ${shortId(participant.campaign_id)}`;
  }
  if (participant.request_id) {
    return `Lead request ${shortId(participant.request_id)}`;
  }
  return "No Campaign assignment recorded";
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
  return (
    <button
      type="button"
      className={cn(
        "agent-card",
        selected && "is-selected",
        `agent-observation-${participant.observation.toLowerCase()}`,
      )}
      aria-pressed={selected}
      aria-label={`Inspect ${ROLE_LABEL[participant.role]} ${participant.participant_id}, status ${participant.status}, observation ${observationLabel(participant)}`}
      onClick={() => onSelect(participant.participant_id)}
      data-testid={`agent-card-${participant.participant_id}`}
    >
      <span className="agent-card-top">
        <span className="agent-card-role">{ROLE_LABEL[participant.role]}</span>
        <StatusBadge status={participant.status} />
      </span>
      <span className="agent-card-id">
        <code>{shortId(participant.identity_id, 18)}</code>
      </span>
      <span className="agent-card-route">{routeLabel(participant)}</span>
      <span className="agent-card-assignment">
        {assignmentLabel(participant)}
      </span>
      <span className="agent-card-footer">
        <StatusBadge
          status={participant.observation}
          label={observationLabel(participant)}
        />
        <time dateTime={participant.last_observed_at}>
          {formatTimestamp(participant.last_observed_at)}
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
                <h3 id={`agent-zone-${zone.id}`}>{zone.label}</h3>
                <p className="muted">{zone.description}</p>
              </div>
              <span className="agent-zone-count">
                {members.length} recorded
              </span>
            </header>
            {members.length === 0 ? (
              <EmptyState
                title={`No recorded ${ROLE_LABEL[zone.role].toLowerCase()}`}
                description="Core has no participant record for this role in the current projection."
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
  if (participants.length === 0) {
    return (
      <EmptyState
        title="No participants"
        description="No participant matches the current authoritative filters."
      />
    );
  }
  return (
    <div className="agent-table-wrap">
      <table data-testid="agent-table">
        <thead>
          <tr>
            <th>Role / participant</th>
            <th>Status</th>
            <th>Observation</th>
            <th>Route</th>
            <th>Assignment</th>
            <th>Session</th>
            <th>Last observed</th>
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
              <th scope="row">
                <button
                  type="button"
                  className="link-button"
                  aria-pressed={participant.participant_id === selectedId}
                  onClick={() => onSelect(participant.participant_id)}
                >
                  {ROLE_LABEL[participant.role]}{" "}
                  <code>{shortId(participant.identity_id, 18)}</code>
                </button>
                <div className="muted">
                  {participant.identity_kind.replaceAll("_", " ")}
                </div>
              </th>
              <td>
                <StatusBadge status={participant.status} />
              </td>
              <td>
                <StatusBadge
                  status={participant.observation}
                  label={observationLabel(participant)}
                />
                <div className="muted">{participant.lease_state}</div>
              </td>
              <td>{routeLabel(participant)}</td>
              <td>{assignmentLabel(participant)}</td>
              <td>
                {participant.session ? (
                  <>
                    <code>
                      {shortId(participant.session.session_record_id, 14)}
                    </code>
                    <div className="muted">
                      generation {participant.session.generation}
                    </div>
                  </>
                ) : (
                  <span className="muted">No session applies</span>
                )}
              </td>
              <td className="muted">
                <time dateTime={participant.last_observed_at}>
                  {formatTimestamp(participant.last_observed_at)}
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
  if (participants.length === 0) {
    return (
      <EmptyState
        title="No participant observations"
        description="No participant matches the current authoritative filters."
      />
    );
  }
  const ordered = [...participants].sort((left, right) =>
    right.last_observed_at.localeCompare(left.last_observed_at),
  );
  return (
    <div className="agent-timeline-wrap">
      <p className="muted">
        This view orders the same participant records by their last recorded
        observation. It is not an event stream and does not invent activity
        between observations.
      </p>
      <ol className="agent-timeline" data-testid="agent-timeline">
        {ordered.map((participant) => (
          <li key={participant.participant_id}>
            <time dateTime={participant.last_observed_at}>
              {formatTimestamp(participant.last_observed_at)}
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
                <strong>{ROLE_LABEL[participant.role]}</strong>{" "}
                <code>{shortId(participant.identity_id, 18)}</code>
              </span>
              <span className="agent-timeline-meta">
                <StatusBadge status={participant.status} />
                <StatusBadge
                  status={participant.observation}
                  label={observationLabel(participant)}
                />
                <span className="muted">{assignmentLabel(participant)}</span>
              </span>
              <span className="muted">
                Recorded at {formatTimestamp(participant.recorded_at)} · source{" "}
                {participant.status_source}
              </span>
            </button>
          </li>
        ))}
      </ol>
    </div>
  );
}
