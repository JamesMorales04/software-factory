import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  CoreApiError,
  coreApi,
  type ParticipantRecord,
  type ProviderToolEventProjection,
  type ProviderToolEventView,
} from "../../coreApi";
import { shortId } from "../../format";
import { useI18n } from "../../i18n";
import { Button, IconButton } from "../ui/Button";
import { EmptyState, Panel } from "../ui/Surface";
import { ParticipantSessionList } from "./ParticipantSessionList";
import { StatusBadge } from "./StatusBadge";

function roleLabel(
  role: ParticipantRecord["role"],
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

const DETAIL_KEY = {
  RECORDED: "inspector.detail.recorded",
  WITHHELD: "inspector.detail.withheld",
  REDACTED: "inspector.detail.redacted",
  EMPTY: "inspector.detail.empty",
} as const;

function routeText(
  participant: ParticipantRecord,
  t: ReturnType<typeof useI18n>["t"],
): string {
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
  return t("inspector.routeText", { requested, observed });
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
  const { t, formatDate } = useI18n();
  return (
    <div className="agent-tool-events-wrap table-responsive">
      <table
        data-testid="tool-event-table"
        aria-label={t("inspector.toolEvents.table.label")}
      >
        <thead>
          <tr>
            <th>{t("inspector.toolEvents.table.seq")}</th>
            <th>{t("inspector.toolEvents.table.recorded")}</th>
            <th>{t("inspector.toolEvents.table.tool")}</th>
            <th>{t("inspector.toolEvents.table.decision")}</th>
            <th>{t("inspector.toolEvents.table.detail")}</th>
            <th>{t("inspector.toolEvents.table.callRef")}</th>
          </tr>
        </thead>
        <tbody>
          {events.map((event) => (
            <tr key={event.sequence}>
              <td
                className="numeric"
                data-label={t("inspector.toolEvents.table.seq")}
              >
                {event.sequence}
              </td>
              <td
                className="muted"
                data-label={t("inspector.toolEvents.table.recorded")}
              >
                {formatDate(event.recorded_at)}
              </td>
              <td data-label={t("inspector.toolEvents.table.tool")}>
                <code>{event.tool}</code>
                {event.tool === "OTHER" && (
                  <div className="muted">
                    {t("inspector.toolEvents.digest", {
                      id: shortId(event.tool_digest, 12),
                    })}
                  </div>
                )}
              </td>
              <td data-label={t("inspector.toolEvents.table.decision")}>
                <StatusBadge status={event.decision} />
              </td>
              <td data-label={t("inspector.toolEvents.table.detail")}>
                {event.detail ?? (
                  <span className="muted">
                    {DETAIL_KEY[event.detail_state as keyof typeof DETAIL_KEY]
                      ? t(
                          DETAIL_KEY[
                            event.detail_state as keyof typeof DETAIL_KEY
                          ],
                        )
                      : event.detail_state}
                  </span>
                )}
              </td>
              <td data-label={t("inspector.toolEvents.table.callRef")}>
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
  const { t } = useI18n();
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
            : t("inspector.toolEvents.error"),
        );
      })
      .finally(() => {
        if (current === sequence.current) setLoading(false);
      });
    return () => {
      sequence.current += 1;
    };
  }, [sessionId, t]);

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
            : t("inspector.toolEvents.error"),
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
            : t("inspector.toolEvents.errorMore"),
        );
      })
      .finally(() => {
        if (current === sequence.current) setLoadingMore(false);
      });
  };

  return (
    <section className="agent-inspector-section" aria-labelledby="tool-events">
      <div className="section-heading">
        <h4 id="tool-events">{t("inspector.toolEvents.title")}</h4>
        <Button size="sm" onClick={retry} disabled={loading}>
          {t("inspector.toolEvents.refresh")}
        </Button>
      </div>
      <p className="muted">{t("inspector.toolEvents.description")}</p>
      {loading && <p className="muted">{t("inspector.toolEvents.loading")}</p>}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {!loading && !error && events.length === 0 && (
        <EmptyState
          title={t("inspector.toolEvents.emptyTitle")}
          description={t("inspector.toolEvents.emptyDescription")}
        />
      )}
      {events.length > 0 && <ToolEventTable events={events} />}
      {projection?.discarded ? (
        <p className="warning">
          {t(
            projection.discarded === 1
              ? "inspector.toolEvents.discardedOne"
              : "inspector.toolEvents.discardedOther",
            { count: projection.discarded },
          )}
        </p>
      ) : null}
      {projection?.has_more && (
        <Button size="sm" onClick={loadMore} disabled={loadingMore}>
          {loadingMore
            ? t("inspector.toolEvents.loadingMore")
            : t("inspector.toolEvents.loadMore")}
        </Button>
      )}
      {projection && !projection.has_more && events.length > 0 && (
        <p className="muted">{t("inspector.toolEvents.end")}</p>
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
  const { t, formatDate, formatNumber, formatMicros } = useI18n();
  if (!participant) {
    return (
      <Panel className="agent-inspector" as="aside">
        <EmptyState
          title={t("inspector.selectTitle")}
          description={t("inspector.selectDescription")}
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
          <p className="muted">{t("inspector.title")}</p>
          <h3>
            {roleLabel(participant.role, t)}{" "}
            <code>{shortId(participant.identity_id, 18)}</code>
          </h3>
        </div>
        <IconButton label={t("inspector.close")} onClick={onClose}>
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
        <h4 id="assignment">{t("inspector.assignment")}</h4>
        <dl className="agent-facts">
          <div>
            <dt>{t("inspector.participant")}</dt>
            <dd>
              <code>{participant.participant_id}</code>
            </dd>
          </div>
          <div>
            <dt>{t("inspector.principal")}</dt>
            <dd>
              {participant.principal_id ? (
                <code>{participant.principal_id}</code>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>{t("inspector.project")}</dt>
            <dd>
              {participant.project_id ? (
                <Link to="/projects">{participant.project_id}</Link>
              ) : (
                "NOT EXPOSED"
              )}
            </dd>
          </div>
          <div>
            <dt>{t("inspector.action")}</dt>
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
            <dt>{t("inspector.campaign")}</dt>
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
            <dt>{t("inspector.workUnit")}</dt>
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
            <dt>{t("inspector.run")}</dt>
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
            <dt>{t("inspector.attempt")}</dt>
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
            <dt>{t("inspector.attemptGeneration")}</dt>
            <dd>{participant.attempt_generation ?? "NOT APPLICABLE"}</dd>
          </div>
          <div>
            <dt>{t("inspector.worker")}</dt>
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
            <dt>{t("inspector.leadRequest")}</dt>
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
            <dt>{t("inspector.modelCalls")}</dt>
            <dd>
              {participant.model_call_count === null
                ? "NOT APPLICABLE"
                : [
                    t("inspector.modelCallsRecorded", {
                      count: formatNumber(participant.model_call_count),
                    }),
                    participant.model_call_state
                      ? t("inspector.modelCallsLatest", {
                          state: participant.model_call_state,
                        })
                      : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
            </dd>
          </div>
        </dl>
      </section>

      <section className="agent-inspector-section" aria-labelledby="route">
        <h4 id="route">{t("inspector.route")}</h4>
        <p>{routeText(participant, t)}</p>
        <p className="muted">
          {t("inspector.provenance", {
            value: participant.route_provenance,
          })}
        </p>
      </section>

      {participant.session && (
        <section className="agent-inspector-section" aria-labelledby="session">
          <h4 id="session">{t("inspector.session")}</h4>
          <ParticipantSessionList
            sessions={[participant.session]}
            showAttempt={false}
            emptyTitle={t("sessions.emptyTitle")}
            emptyDescription={t("sessions.emptyDescription")}
          />
          <dl className="agent-facts">
            <div>
              <dt>{t("inspector.externalSession")}</dt>
              <dd>
                {participant.session.provider_session_id ? (
                  <code>{participant.session.provider_session_id}</code>
                ) : (
                  "NOT EXPOSED"
                )}
              </dd>
            </div>
            <div>
              <dt>{t("inspector.created")}</dt>
              <dd>{formatDate(participant.session.created_at)}</dd>
            </div>
            <div>
              <dt>{t("inspector.updated")}</dt>
              <dd>{formatDate(participant.session.updated_at)}</dd>
            </div>
          </dl>
        </section>
      )}

      <section className="agent-inspector-section" aria-labelledby="usage">
        <h4 id="usage">{t("inspector.usage")}</h4>
        {participant.usage ? (
          participant.usage.usage_known ? (
            <dl className="agent-facts">
              <div>
                <dt>{t("inspector.input")}</dt>
                <dd className="numeric">
                  {formatNumber(participant.usage.input_tokens)}
                </dd>
              </div>
              <div>
                <dt>{t("inspector.output")}</dt>
                <dd className="numeric">
                  {formatNumber(participant.usage.output_tokens)}
                </dd>
              </div>
              <div>
                <dt>{t("inspector.totalTokens")}</dt>
                <dd className="numeric">
                  {formatNumber(participant.usage.total_tokens)}
                </dd>
              </div>
              <div>
                <dt>{t("inspector.measuredCost")}</dt>
                <dd className="numeric">
                  {formatMicros(participant.usage.cost_micros)}
                </dd>
              </div>
            </dl>
          ) : (
            <p>
              <StatusBadge status="UNMEASURED" />
              <span className="muted"> {t("inspector.usageUnknown")}</span>
            </p>
          )
        ) : (
          <p className="muted">{t("inspector.noUsage")}</p>
        )}
        <p className="muted">
          {t("inspector.provenance", {
            value: participant.usage_provenance,
          })}
        </p>
      </section>

      <section className="agent-inspector-section" aria-labelledby="evidence">
        <h4 id="evidence">{t("inspector.evidence")}</h4>
        {evidence ? (
          <>
            <dl className="agent-facts">
              <div>
                <dt>{t("inspector.candidate")}</dt>
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
                <dt>{t("inspector.commit")}</dt>
                <dd>
                  {evidence.candidate_commit ? (
                    <code>{evidence.candidate_commit}</code>
                  ) : (
                    "NOT RECORDED"
                  )}
                </dd>
              </div>
              <div>
                <dt>{t("inspector.tree")}</dt>
                <dd>
                  {evidence.candidate_tree ? (
                    <code>{evidence.candidate_tree}</code>
                  ) : (
                    "NOT RECORDED"
                  )}
                </dd>
              </div>
              <div>
                <dt>{t("inspector.gate")}</dt>
                <dd>
                  {evidence.gate_outcome ? (
                    <Link to={campaignTab(participant, "gate")}>
                      <StatusBadge status={evidence.gate_outcome} />
                    </Link>
                  ) : (
                    <StatusBadge
                      status="PENDING"
                      label={t("inspector.noGateRecorded")}
                    />
                  )}
                </dd>
              </div>
              <div>
                <dt>{t("inspector.verifierReceipt")}</dt>
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
                <dt>{t("inspector.reviewerReceipt")}</dt>
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
                {t("inspector.openWorkGraph")}
              </Link>
              <Link to={campaignTab(participant, "execution")}>
                {t("inspector.openExecution")}
              </Link>
              <Link to={campaignTab(participant, "evidence")}>
                {t("inspector.openEvidence")}
              </Link>
            </div>
          </>
        ) : (
          <p className="muted">{t("inspector.noEvidence")}</p>
        )}
      </section>

      {participant.failure_reason && (
        <section className="agent-inspector-section" aria-labelledby="failure">
          <h4 id="failure">{t("inspector.error")}</h4>
          <p className="error">{participant.failure_reason}</p>
          {participant.session && (
            <p className="muted">
              {t("inspector.cleanup", {
                cleanup: [
                  participant.session.cleanup,
                  participant.session.cleanup_receipt,
                ]
                  .filter(Boolean)
                  .join(" · "),
              })}
            </p>
          )}
        </section>
      )}

      <section className="agent-inspector-section" aria-labelledby="timestamps">
        <h4 id="timestamps">{t("inspector.timestamps")}</h4>
        <dl className="agent-facts">
          <div>
            <dt>{t("inspector.recorded")}</dt>
            <dd>{formatDate(participant.recorded_at)}</dd>
          </div>
          <div>
            <dt>{t("inspector.lastObserved")}</dt>
            <dd>{formatDate(participant.last_observed_at)}</dd>
          </div>
        </dl>
      </section>

      <ToolEvents participant={participant} />
    </Panel>
  );
}
