import { useState } from "react";
import {
  CoreApiError,
  coreApi,
  type IssueDeliveryEvent,
  type IssueDeliveryState,
  type IssueLinkProjection,
  type IssueLinkState,
} from "../../coreApi";
import { formatTimestamp, shortId } from "../../format";
import { useI18n } from "../../i18n";
import { Badge, type BadgeTone } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Panel } from "../ui/Surface";

function stateTone(state: IssueLinkState): BadgeTone {
  switch (state) {
    case "DONE":
      return "success";
    case "IN_REVIEW":
      return "info";
    case "NEEDS_ATTENTION":
      return "attention";
    case "IN_PROGRESS":
      return "accent";
    case "QUEUED":
      return "info";
    default:
      return "neutral";
  }
}

function deliveryTone(state: IssueDeliveryState): BadgeTone {
  switch (state) {
    case "CLOSED":
      return "success";
    case "COMMENTED":
      return "info";
    case "PARTIAL":
      return "attention";
    case "FAILED":
    case "BLOCKED":
      return "danger";
    case "PENDING":
      return "attention";
    default:
      return "neutral";
  }
}

function eventTone(event: IssueDeliveryEvent): BadgeTone {
  if (event.status === "APPLIED") return "success";
  if (event.status === "FAILED" || event.status === "BLOCKED") return "danger";
  return "attention";
}

function canRetry(event: IssueDeliveryEvent): boolean {
  return event.status === "FAILED" || event.status === "BLOCKED";
}

export function LinkedIssues({
  links,
  onRetried,
}: {
  links?: IssueLinkProjection[] | null;
  onRetried?: () => void;
}) {
  const { t } = useI18n();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const items = links ?? [];

  async function retry(link: IssueLinkProjection, event: IssueDeliveryEvent) {
    const key = `${link.id}:${event.id}`;
    setBusy(key);
    setError(null);
    try {
      await coreApi.retryIssueDelivery(link.id, event.id);
      onRetried?.();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("linkedIssues.retryError"),
      );
    } finally {
      setBusy(null);
    }
  }

  return (
    <Panel className="linked-issues">
      <div className="section-heading">
        <h2>{t("linkedIssues.title")}</h2>
        <span className="muted">
          {items.length === 0
            ? t("linkedIssues.empty")
            : t("linkedIssues.count", { count: items.length })}
        </span>
      </div>
      <p className="muted">{t("linkedIssues.description")}</p>
      {error && (
        <p className="ui-callout ui-callout--danger" role="alert">
          {error}
        </p>
      )}
      {items.length > 0 && (
        <ul className="linked-issue-list">
          {items.map((link) => (
            <li key={link.id} className="linked-issue">
              <div className="linked-issue-head">
                <div>
                  <strong>
                    {link.repositoryFullName}#{link.issueNumber}
                  </strong>{" "}
                  <span>{link.issueTitle}</span>
                </div>
                <div className="row">
                  <Badge tone={link.role === "PRIMARY" ? "accent" : "neutral"}>
                    {t(
                      link.role === "PRIMARY"
                        ? "linkedIssues.rolePrimary"
                        : "linkedIssues.roleRelated",
                    )}
                  </Badge>
                  <Badge tone={stateTone(link.state)}>{link.state}</Badge>
                  <Badge tone={deliveryTone(link.deliveryState)}>
                    {t("linkedIssues.delivery")}: {link.deliveryState}
                  </Badge>
                </div>
              </div>
              <dl className="linked-issue-facts">
                {link.workUnitId && (
                  <div>
                    <dt>{t("linkedIssues.workUnit")}</dt>
                    <dd>
                      <code>{link.workUnitId}</code>
                    </dd>
                  </div>
                )}
                {link.pullRequestNumber !== null && (
                  <div>
                    <dt>{t("linkedIssues.pullRequest")}</dt>
                    <dd>
                      {link.pullRequestUrl ? (
                        <a
                          href={link.pullRequestUrl}
                          target="_blank"
                          rel="noreferrer"
                        >
                          #{link.pullRequestNumber}
                        </a>
                      ) : (
                        `#${link.pullRequestNumber}`
                      )}
                    </dd>
                  </div>
                )}
                {link.mergeSha && (
                  <div>
                    <dt>{t("linkedIssues.merge")}</dt>
                    <dd>
                      <code>{shortId(link.mergeSha, 12)}</code>
                      {link.mergedAt
                        ? ` · ${formatTimestamp(link.mergedAt)}`
                        : ""}
                    </dd>
                  </div>
                )}
              </dl>
              {link.latestError && (
                <p className="ui-callout ui-callout--attention" role="status">
                  <strong>{link.latestError.kind}</strong> —{" "}
                  {link.latestError.detail}
                </p>
              )}
              <details className="linked-issue-receipts">
                <summary>
                  {t("linkedIssues.receipts", { count: link.events.length })}
                </summary>
                {link.events.length === 0 ? (
                  <p className="muted">{t("linkedIssues.noReceipts")}</p>
                ) : (
                  <ul>
                    {link.events.map((event) => (
                      <li key={event.id}>
                        <span>
                          {t("linkedIssues.event")} {event.action} ·{" "}
                          {event.eventKey}
                        </span>
                        <Badge tone={eventTone(event)}>{event.status}</Badge>
                        <span className="muted">
                          {t("linkedIssues.attempts", {
                            count: event.attempts,
                          })}
                          {event.appliedAt
                            ? ` · ${formatTimestamp(event.appliedAt)}`
                            : ""}
                        </span>
                        {event.errorDetail && (
                          <span className="linked-issue-error">
                            {event.errorKind}: {event.errorDetail}
                          </span>
                        )}
                        {canRetry(event) && (
                          <Button
                            size="sm"
                            variant="secondary"
                            disabled={busy !== null}
                            onClick={() => void retry(link, event)}
                          >
                            {busy === `${link.id}:${event.id}`
                              ? t("linkedIssues.retrying")
                              : t("linkedIssues.retry")}
                          </Button>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
              </details>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}
