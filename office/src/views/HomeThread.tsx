import { ArrowRight } from "lucide-react";
import { Link } from "react-router-dom";
import { Panel } from "../components/ui/Surface";
import type {
  IntakeRequestProjection,
  IntakeRequestSummary,
  IntakeState,
} from "../coreApi";
import { type MessageKey, useI18n } from "../i18n";
import { IntakeStateBadge } from "./RequestState";

const STATE_STATUS: Record<IntakeState, MessageKey> = {
  SUBMITTED: "home.thread.state.SUBMITTED",
  ASSESSING: "home.thread.state.ASSESSING",
  PLAN_READY: "home.thread.state.PLAN_READY",
  AWAITING_APPROVAL: "home.thread.state.AWAITING_APPROVAL",
  EXECUTING: "home.thread.state.EXECUTING",
  BLOCKED: "home.thread.state.BLOCKED",
  FAILED: "home.thread.state.FAILED",
  COMPLETED: "home.thread.state.COMPLETED",
  CANCELLED: "home.thread.state.CANCELLED",
};

function latestMessage(
  projection: IntakeRequestProjection | null,
): string | null {
  if (projection === null || projection.messages.length === 0) return null;
  const message = projection.messages[projection.messages.length - 1];
  return message.body.trim() === "" ? null : message.body;
}

/**
 * Shows the latest Core-projected request as a short Lead thread. The card
 * never invents a reply: the status is derived from the request state and the
 * only quoted text comes from durable Core messages.
 */
export function HomeThread({
  summary,
  projection,
  projectName,
  loading,
  error,
}: {
  summary: IntakeRequestSummary | null;
  projection: IntakeRequestProjection | null;
  projectName: string;
  loading: boolean;
  error: string | null;
}) {
  const { t, formatDate } = useI18n();
  if (summary === null) return null;

  const note = latestMessage(projection);
  const showNote = note !== null && note.trim() !== summary.objective.trim();
  const nextAction = projection?.assessment?.suggestedNextAction?.trim();
  const stateDetail =
    projection?.stateDetail?.trim() || summary.stateDetail?.trim() || null;
  const status =
    (summary.state === "BLOCKED" || summary.state === "FAILED") && stateDetail
      ? stateDetail
      : t(STATE_STATUS[summary.state]);

  return (
    <Panel className="home-thread" data-testid="home-thread">
      <div className="home-thread-head">
        <div className="home-thread-title">
          <span className="home-thread-avatar" aria-hidden="true">
            L
          </span>
          <div className="home-thread-heading">
            <span className="home-kicker">{t("home.thread.kicker")}</span>
            <h2>{t("home.thread.title")}</h2>
          </div>
        </div>
        <IntakeStateBadge state={summary.state} />
      </div>

      {loading && projection === null ? (
        <div className="home-thread-skeleton" role="status">
          <span className="ui-visually-hidden">{t("home.thread.loading")}</span>
          <span aria-hidden="true" />
          <span aria-hidden="true" />
          <span aria-hidden="true" />
        </div>
      ) : (
        <div className="home-thread-body">
          <div className="home-thread-bubble home-thread-bubble--you">
            <span className="home-thread-role">{t("home.thread.you")}</span>
            <p>{summary.objective}</p>
          </div>
          {showNote && (
            <div className="home-thread-bubble home-thread-bubble--note">
              <span className="home-thread-role">{t("home.thread.you")}</span>
              <p>{note}</p>
            </div>
          )}
          <div className="home-thread-lead">
            <span
              className="home-thread-avatar home-thread-avatar--small"
              aria-hidden="true"
            >
              L
            </span>
            <div className="home-thread-lead-copy">
              <span className="home-thread-role">{t("home.thread.lead")}</span>
              <p>{status}</p>
              {nextAction && (
                <p className="home-thread-next">
                  {t("home.thread.next", { action: nextAction })}
                </p>
              )}
            </div>
          </div>
          {error && (
            <p className="home-thread-error" role="status">
              {t("home.thread.error")}
            </p>
          )}
        </div>
      )}

      <div className="home-thread-foot">
        <span className="home-thread-meta">
          {projectName} · {formatDate(summary.updatedAt)}
        </span>
        <Link
          className="home-thread-continue"
          to={`/requests/${encodeURIComponent(summary.id)}`}
        >
          {t("home.thread.continue")}
          <ArrowRight size={14} aria-hidden="true" />
        </Link>
      </div>
    </Panel>
  );
}
