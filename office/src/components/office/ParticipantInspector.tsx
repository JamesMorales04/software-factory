import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  CoreApiError,
  coreApi,
  type ParticipantRecord,
  type ProviderToolEventProjection,
  type ProviderToolEventView,
} from "../../coreApi";
import { formatCostMicros, formatTimestamp, shortId } from "../../format";
import { Button, IconButton } from "../ui/Button";
import { EmptyState, Panel } from "../ui/Surface";
import { ParticipantSessionList } from "./ParticipantSessionList";
import { StatusBadge } from "./StatusBadge";

const ROLE_LABEL: Record<ParticipantRecord["role"], string> = {
  IMPLEMENTER: "Implementer",
  LEAD: "Lead",
  VERIFIER: "Verifier",
  REVIEWER: "Reviewer",
  WORKER: "Worker",
};

const DETAIL_LABEL: Record<string, string> = {
  RECORDED: "recorded",
  WITHHELD: "provider content withheld",
  REDACTED: "redacted legacy record",
  EMPTY: "no detail recorded",
};

function routeText(participant: ParticipantRecord): string {
  const requested = participant.requested_route
    ? `${participant.requested_route.provider}/${participant.requested_route.model}${
        participant.requested_route.mode
          ? ` (${participant.requested_route.mode})`
          : ""
      }`
    : "NOT EXPOSED";
  const observed = participant.observed_route
    ? `${participant.observed_route.provider}/${participant.observed_route.model}`
    : "NOT EXPOSED";
  return `requested ${requested} · observed ${observed}`;
}

function campaignTab(
  participant: ParticipantRecord,
  tab: string,
  unit?: string | null,
): string {
  const params = new URLSearchParams({ tab });
  if (unit) params.set("unit", unit);
  return `/campaigns/${encodeURIComponent(participant.campaign_id ?? "")}?${params.toString()}`;
}

function ToolEventTable({ events }: { events: ProviderToolEventView[] }) {
  return (
    <div className="agent-tool-events-wrap">
      <table data-testid="tool-event-table">
        <thead>
          <tr>
            <th>Seq</th>
            <th>Recorded</th>
            <th>Tool</th>
            <th>Decision</th>
            <th>Detail</th>
            <th>Call ref</th>
          </tr>
        </thead>
        <tbody>
          {events.map((event) => (
            <tr key={event.sequence}>
              <td className="numeric">{event.sequence}</td>
              <td className="muted">{formatTimestamp(event.recorded_at)}</td>
              <td>
                <code>{event.tool}</code>
                {event.tool === "OTHER" && (
                  <div className="muted">
                    digest {shortId(event.tool_digest, 12)}
                  </div>
                )}
              </td>
              <td>
                <StatusBadge status={event.decision} />
              </td>
              <td>
                {event.detail ?? (
                  <span className="muted">
                    {DETAIL_LABEL[event.detail_state] ?? event.detail_state}
                  </span>
                )}
              </td>
              <td>
                <code>{event.call_ref}</code>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ToolEvents({ participant }: { participant: ParticipantRecord }) {
  const sessionId = participant.session?.session_record_id ?? null;
  const [events, setEvents] = useState<ProviderToolEventView[]>([]);
  const [projection, setProjection] =
    useState<ProviderToolEventProjection | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const sequence = useRef(0);

  useEffect(() => {
    setEvents([]);
    setProjection(null);
    setError(null);
    if (!sessionId) return;
    const current = ++sequence.current;
    setLoading(true);
    void coreApi
      .listToolEvents(sessionId, 0, 100)
      .then((next) => {
        if (current !== sequence.current) return;
        setProjection(next);
        setEvents(next.events);
        setError(null);
      })
      .catch((cause) => {
        if (current !== sequence.current) return;
        setError(
          cause instanceof CoreApiError
            ? cause.message
            : "Failed to load recorded tool events.",
        );
      })
      .finally(() => {
        if (current === sequence.current) setLoading(false);
      });
    return () => {
      sequence.current += 1;
    };
  }, [sessionId]);

  if (!sessionId) return null;

  const retry = () => {
    const current = ++sequence.current;
    setLoading(true);
    setError(null);
    void coreApi
      .listToolEvents(sessionId, 0, 100)
      .then((next) => {
        if (current !== sequence.current) return;
        setProjection(next);
        setEvents(next.events);
      })
      .catch((cause) => {
        if (current !== sequence.current) return;
        setError(
          cause instanceof CoreApiError
            ? cause.message
            : "Failed to load recorded tool events.",
        );
      })
      .finally(() => {
        if (current === sequence.current) setLoading(false);
      });
  };

  const loadMore = () => {
    const after = projection?.next_sequence;
    if (after === null || after === undefined) return;
    const current = sequence.current;
    setLoadingMore(true);
    void coreApi
      .listToolEvents(sessionId, after, 100)
      .then((next) => {
        if (current !== sequence.current) return;
        setProjection(next);
        setEvents((existing) => {
          const seen = new Set(existing.map((event) => event.sequence));
          const merged = [...existing];
          for (const event of next.events) {
            if (!seen.has(event.sequence)) {
              seen.add(event.sequence);
              merged.push(event);
            }
          }
          return merged.sort((left, right) => left.sequence - right.sequence);
        });
        setError(null);
      })
      .catch((cause) => {
        if (current !== sequence.current) return;
        setError(
          cause instanceof CoreApiError
            ? cause.message
            : "Failed to load more tool events.",
        );
      })
      .finally(() => {
        if (current === sequence.current) setLoadingMore(false);
      });
  };

  return (
    <section className="agent-inspector-section" aria-labelledby="tool-events">
      <div className="section-heading">
        <h4 id="tool-events">Recorded tool events</h4>
        <Button size="sm" onClick={retry} disabled={loading}>
          Refresh events
        </Button>
      </div>
      <p className="muted">
        Read-only projection of <code>provider_tool_events</code>. Free-form
        provider detail is withheld unless Core classifies it as safe.
      </p>
      {loading && <p className="muted">Loading recorded tool events…</p>}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {!loading && !error && events.length === 0 && (
        <EmptyState
          title="No recorded tool events"
          description="Core has no provider tool event for this exact session. An empty list is not evidence of inactivity."
        />
      )}
      {events.length > 0 && <ToolEventTable events={events} />}
      {projection?.discarded ? (
        <p className="warning">
          {projection.discarded} malformed or out-of-order record
          {projection.discarded === 1 ? " was" : "s were"} discarded by Core.
        </p>
      ) : null}
      {projection?.has_more && (
        <Button size="sm" onClick={loadMore} disabled={loadingMore}>
          {loadingMore ? "Loading…" : "Load more events"}
        </Button>
      )}
      {projection && !projection.has_more && events.length > 0 && (
        <p className="muted">End of the recorded event projection.</p>
      )}
    </section>
  );
}

export function ParticipantInspector({
  participant,
  onClose,
}: {
  participant: ParticipantRecord | null;
  onClose: () => void;
}) {
  if (!participant) {
    return (
      <Panel className="agent-inspector" as="aside">
        <EmptyState
          title="Select a participant"
          description="Choose a workstation, table row or timeline entry to inspect its authoritative Core records."
        />
      </Panel>
    );
  }

  const evidence = participant.evidence;
  return (
    <Panel
      className="agent-inspector"
      as="aside"
      data-testid="participant-inspector"
    >
      <div className="agent-inspector-header">
        <div>
          <p className="muted">Participant inspector</p>
          <h3>
            {ROLE_LABEL[participant.role]}{" "}
            <code>{shortId(participant.identity_id, 18)}</code>
          </h3>
        </div>
        <IconButton label="Close participant inspector" onClick={onClose}>
          ×
        </IconButton>
      </div>

      <div className="agent-inspector-badges">
        <StatusBadge status={participant.status} />
        <StatusBadge status={participant.observation} />
        <StatusBadge status={participant.lease_state} />
        <span className="muted">{participant.identity_kind}</span>
      </div>

      <p className="muted">{participant.observation_note}</p>

      <section className="agent-inspector-section" aria-labelledby="assignment">
        <h4 id="assignment">Assignment and identity</h4>
        <dl className="agent-facts">
          <div>
            <dt>Participant</dt>
            <dd>
              <code>{participant.participant_id}</code>
            </dd>
          </div>
          <div>
            <dt>Principal</dt>
            <dd>
              {participant.principal_id ? (
                <code>{participant.principal_id}</code>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>Project</dt>
            <dd>
              {participant.project_id ? (
                <Link to="/projects">{participant.project_id}</Link>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>Action</dt>
            <dd>
              {participant.action_id ? (
                <Link
                  to={`/actions/${encodeURIComponent(participant.action_id)}`}
                >
                  <code>{shortId(participant.action_id, 14)}</code>
                </Link>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>Campaign</dt>
            <dd>
              {participant.campaign_id ? (
                <Link to={campaignTab(participant, "overview")}>
                  <code>{participant.campaign_id}</code>
                </Link>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>Work unit</dt>
            <dd>
              {participant.work_unit_id ? (
                <Link
                  to={campaignTab(
                    participant,
                    "workgraph",
                    participant.work_unit_id,
                  )}
                >
                  <code>{participant.work_unit_id}</code>
                </Link>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>Run</dt>
            <dd>
              {participant.run_id ? (
                <Link to={campaignTab(participant, "execution")}>
                  <code>{participant.run_id}</code>
                </Link>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>Attempt</dt>
            <dd>
              {participant.attempt_id ? (
                <Link to={campaignTab(participant, "execution")}>
                  <code>{participant.attempt_id}</code>
                </Link>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>Attempt generation</dt>
            <dd>{participant.attempt_generation ?? "NOT APPLICABLE"}</dd>
          </div>
          <div>
            <dt>Worker</dt>
            <dd>
              {participant.worker_id ? (
                <Link to="/workers">
                  <code>{participant.worker_id}</code>
                </Link>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>Lead request</dt>
            <dd>
              {participant.request_id ? (
                <Link to="/lead">
                  <code>{shortId(participant.request_id, 18)}</code>
                </Link>
              ) : (
                "NOT APPLICABLE"
              )}
            </dd>
          </div>
          <div>
            <dt>Model calls</dt>
            <dd>
              {participant.model_call_count === null
                ? "NOT APPLICABLE"
                : `${participant.model_call_count} recorded${
                    participant.model_call_state
                      ? ` · latest ${participant.model_call_state}`
                      : ""
                  }`}
            </dd>
          </div>
        </dl>
      </section>

      <section className="agent-inspector-section" aria-labelledby="route">
        <h4 id="route">Provider route</h4>
        <p>{routeText(participant)}</p>
        <p className="muted">Provenance: {participant.route_provenance}</p>
      </section>

      {participant.session && (
        <section className="agent-inspector-section" aria-labelledby="session">
          <h4 id="session">Provider session</h4>
          <ParticipantSessionList
            sessions={[participant.session]}
            showAttempt={false}
            emptyTitle="No provider session"
            emptyDescription="This participant has no recorded provider session."
          />
          <dl className="agent-facts">
            <div>
              <dt>External session</dt>
              <dd>
                {participant.session.provider_session_id ? (
                  <code>{participant.session.provider_session_id}</code>
                ) : (
                  "NOT EXPOSED"
                )}
              </dd>
            </div>
            <div>
              <dt>Created</dt>
              <dd>{formatTimestamp(participant.session.created_at)}</dd>
            </div>
            <div>
              <dt>Updated</dt>
              <dd>{formatTimestamp(participant.session.updated_at)}</dd>
            </div>
          </dl>
        </section>
      )}

      <section className="agent-inspector-section" aria-labelledby="usage">
        <h4 id="usage">Usage</h4>
        {participant.usage ? (
          participant.usage.usage_known ? (
            <dl className="agent-facts">
              <div>
                <dt>Input</dt>
                <dd className="numeric">
                  {participant.usage.input_tokens.toLocaleString()}
                </dd>
              </div>
              <div>
                <dt>Output</dt>
                <dd className="numeric">
                  {participant.usage.output_tokens.toLocaleString()}
                </dd>
              </div>
              <div>
                <dt>Total tokens</dt>
                <dd className="numeric">
                  {participant.usage.total_tokens.toLocaleString()}
                </dd>
              </div>
              <div>
                <dt>Measured cost</dt>
                <dd className="numeric">
                  {formatCostMicros(participant.usage.cost_micros)}
                </dd>
              </div>
            </dl>
          ) : (
            <p>
              <StatusBadge status="UNMEASURED" />
              <span className="muted">
                {" "}
                Core records this usage as unknown, not zero.
              </span>
            </p>
          )
        ) : (
          <p className="muted">No usage applies to this participant record.</p>
        )}
        <p className="muted">Provenance: {participant.usage_provenance}</p>
      </section>

      <section className="agent-inspector-section" aria-labelledby="evidence">
        <h4 id="evidence">Quality evidence</h4>
        {evidence ? (
          <>
            <dl className="agent-facts">
              <div>
                <dt>Candidate</dt>
                <dd>
                  {evidence.candidate_id ? (
                    <Link to={campaignTab(participant, "candidates")}>
                      <code>{evidence.candidate_id}</code>
                    </Link>
                  ) : (
                    "NOT RECORDED"
                  )}
                </dd>
              </div>
              <div>
                <dt>Commit</dt>
                <dd>
                  {evidence.candidate_commit ? (
                    <code>{evidence.candidate_commit}</code>
                  ) : (
                    "NOT RECORDED"
                  )}
                </dd>
              </div>
              <div>
                <dt>Tree</dt>
                <dd>
                  {evidence.candidate_tree ? (
                    <code>{evidence.candidate_tree}</code>
                  ) : (
                    "NOT RECORDED"
                  )}
                </dd>
              </div>
              <div>
                <dt>Quality Gate</dt>
                <dd>
                  {evidence.gate_outcome ? (
                    <Link to={campaignTab(participant, "gate")}>
                      <StatusBadge status={evidence.gate_outcome} />
                    </Link>
                  ) : (
                    <StatusBadge status="PENDING" label="NO GATE RECORDED" />
                  )}
                </dd>
              </div>
              <div>
                <dt>Verifier receipt</dt>
                <dd>
                  {evidence.verifier_receipt_id ? (
                    <Link to={campaignTab(participant, "evidence")}>
                      <code>{shortId(evidence.verifier_receipt_id, 18)}</code>
                    </Link>
                  ) : (
                    "PENDING"
                  )}
                </dd>
              </div>
              <div>
                <dt>Reviewer receipt</dt>
                <dd>
                  {evidence.reviewer_receipt_id ? (
                    <Link to={campaignTab(participant, "evidence")}>
                      <code>{shortId(evidence.reviewer_receipt_id, 18)}</code>
                    </Link>
                  ) : (
                    "PENDING"
                  )}
                </dd>
              </div>
            </dl>
            <div className="agent-inspector-links">
              <Link to={campaignTab(participant, "workgraph")}>
                Open WorkGraph
              </Link>
              <Link to={campaignTab(participant, "execution")}>
                Open execution
              </Link>
              <Link to={campaignTab(participant, "evidence")}>
                Open evidence
              </Link>
            </div>
          </>
        ) : (
          <p className="muted">
            No candidate or quality evidence is bound to this participant
            record.
          </p>
        )}
      </section>

      {participant.failure_reason && (
        <section className="agent-inspector-section" aria-labelledby="failure">
          <h4 id="failure">Error and cleanup</h4>
          <p className="error">{participant.failure_reason}</p>
          {participant.session && (
            <p className="muted">
              Cleanup: {participant.session.cleanup}
              {participant.session.cleanup_receipt
                ? ` · ${participant.session.cleanup_receipt}`
                : ""}
            </p>
          )}
        </section>
      )}

      <section className="agent-inspector-section" aria-labelledby="timestamps">
        <h4 id="timestamps">Recorded timestamps</h4>
        <dl className="agent-facts">
          <div>
            <dt>Recorded</dt>
            <dd>{formatTimestamp(participant.recorded_at)}</dd>
          </div>
          <div>
            <dt>Last observed</dt>
            <dd>{formatTimestamp(participant.last_observed_at)}</dd>
          </div>
        </dl>
      </section>

      <ToolEvents participant={participant} />
    </Panel>
  );
}
