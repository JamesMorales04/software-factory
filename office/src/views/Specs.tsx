import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  type AgenticFrozenSpec,
  type AgenticSpecBoard,
  type AgenticSpecBoardCard,
  type AgenticSpecDetail,
  CoreApiError,
  coreApi,
} from "../coreApi";
import { shortId } from "../format";
import { type MessageKey, useI18n } from "../i18n";
import "./specs.css";

function errorText(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

const COLUMN_LABEL: Record<string, MessageKey> = {
  FROZEN: "specs.column.frozen",
  QUEUED: "specs.column.queued",
  RUNNING: "specs.column.running",
  ACCEPTED: "specs.column.accepted",
  BLOCKED: "specs.column.blocked",
  CANCELLED: "specs.column.cancelled",
};

function columnTone(column: string) {
  switch (column) {
    case "ACCEPTED":
      return "success" as const;
    case "RUNNING":
      return "info" as const;
    case "BLOCKED":
      return "danger" as const;
    case "CANCELLED":
      return "neutral" as const;
    default:
      return "attention" as const;
  }
}

function publicationLabel(status: string | null): MessageKey {
  switch (status) {
    case "PUBLISHED":
      return "specs.publication.published";
    case "LOCAL":
      return "specs.publication.local";
    case "FAILED":
      return "specs.publication.failed";
    case "PENDING":
      return "specs.publication.pending";
    default:
      return "specs.publication.unpublished";
  }
}

function publicationTone(status: string | null) {
  switch (status) {
    case "PUBLISHED":
      return "success" as const;
    case "LOCAL":
      return "neutral" as const;
    case "FAILED":
      return "danger" as const;
    default:
      return "attention" as const;
  }
}

function Traceability({ spec }: { spec: AgenticFrozenSpec }) {
  const { t } = useI18n();
  return (
    <div className="specs-traceability">
      {spec.requirements.map((requirement) => (
        <div key={requirement.id} className="specs-requirement">
          <div className="specs-requirement-heading">
            <code>{requirement.id}</code>
            <span>{requirement.statement}</span>
          </div>
          <ul>
            {requirement.assertions.map((assertion) => (
              <li key={assertion.id}>
                <Badge
                  tone={
                    assertion.kind === "ACCEPTANCE" ? "success" : "attention"
                  }
                >
                  {t(
                    assertion.kind === "ACCEPTANCE"
                      ? "specs.assertion.acceptance"
                      : "specs.assertion.negative",
                  )}
                </Badge>
                <code>{assertion.id}</code>
                <span>{assertion.statement}</span>
                <span className="muted">
                  {t("specs.trace.check", { check: assertion.checkId })}
                </span>
              </li>
            ))}
          </ul>
        </div>
      ))}
    </div>
  );
}

export function Specs() {
  const { t, formatDate } = useI18n();
  const [board, setBoard] = useState<AgenticSpecBoard | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<AgenticSpecDetail | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [confirmPublish, setConfirmPublish] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const loadBoard = useCallback(async () => {
    try {
      const next = await coreApi.getAgenticSpecBoard();
      setBoard(next);
      setError(null);
    } catch (cause) {
      setError(errorText(cause, t("specs.error.load")));
    }
  }, [t]);

  useEffect(() => {
    void loadBoard();
    const timer = window.setInterval(() => void loadBoard(), 8000);
    return () => window.clearInterval(timer);
  }, [loadBoard]);

  const selected: AgenticSpecBoardCard | null = useMemo(() => {
    if (!board) return null;
    return board.specs.find((spec) => spec.specId === selectedId) ?? null;
  }, [board, selectedId]);

  useEffect(() => {
    if (!selected) {
      setDetail(null);
      return;
    }
    let active = true;
    setDetailError(null);
    coreApi
      .getAgenticSpec(selected.requestId)
      .then((next) => {
        if (active) setDetail(next);
      })
      .catch((cause) => {
        if (active) {
          setDetail(null);
          setDetailError(errorText(cause, t("specs.error.detail")));
        }
      });
    return () => {
      active = false;
    };
  }, [selected, t]);

  async function refresh() {
    if (!selected) return;
    setBusy("refresh");
    setActionError(null);
    setNotice(null);
    try {
      const result = await coreApi.refreshAgenticSpec(selected.requestId);
      setNotice(
        result.inSync ? t("specs.refresh.inSync") : t("specs.refresh.drifted"),
      );
      await loadBoard();
    } catch (cause) {
      setActionError(errorText(cause, t("specs.refresh.error")));
    } finally {
      setBusy(null);
    }
  }

  async function publish(local: boolean) {
    if (!selected) return;
    if (!local && !confirmPublish) {
      setActionError(t("specs.publish.confirmRequired"));
      return;
    }
    setBusy(local ? "local" : "publish");
    setActionError(null);
    setNotice(null);
    try {
      const result = await coreApi.publishAgenticSpec(
        selected.requestId,
        true,
        local,
      );
      setNotice(
        result.local ? t("specs.publish.localDone") : t("specs.publish.done"),
      );
      setConfirmPublish(false);
      await loadBoard();
    } catch (cause) {
      setActionError(errorText(cause, t("specs.publish.error")));
    } finally {
      setBusy(null);
    }
  }

  if (error) {
    return (
      <div className="page">
        <PageHeader
          title={t("specs.title")}
          description={t("specs.description")}
        />
        <ErrorState
          message={error}
          action={
            <Button variant="secondary" onClick={() => void loadBoard()}>
              {t("specs.retry")}
            </Button>
          }
        />
      </div>
    );
  }

  if (!board) {
    return (
      <div className="page">
        <PageHeader
          title={t("specs.title")}
          description={t("specs.description")}
        />
        <LoadingState label={t("specs.loading")} />
      </div>
    );
  }

  return (
    <div className="page">
      <PageHeader
        title={t("specs.title")}
        description={t("specs.description")}
        actions={
          <Link className="button button--secondary button--md" to="/director">
            {t("specs.openDirector")}
          </Link>
        }
      />

      {board.specs.length === 0 ? (
        <EmptyState
          title={t("specs.empty.title")}
          description={t("specs.empty.description")}
          action={
            <Link className="button button--primary button--md" to="/director">
              {t("specs.openDirector")}
            </Link>
          }
        />
      ) : (
        <section className="specs-board" aria-label={t("specs.board.label")}>
          {board.columns.map((column) => {
            const cards = board.specs.filter((spec) => spec.column === column);
            return (
              <section className="specs-column" key={column}>
                <div className="specs-column-heading">
                  <Badge tone={columnTone(column)}>
                    {t(COLUMN_LABEL[column] ?? "specs.column.frozen")}
                  </Badge>
                  <span className="muted">{cards.length}</span>
                </div>
                <div className="specs-column-cards">
                  {cards.map((card) => (
                    <button
                      key={card.specId}
                      type="button"
                      className={`specs-card ${
                        card.specId === selectedId ? "specs-card--active" : ""
                      }`}
                      onClick={() => setSelectedId(card.specId)}
                    >
                      <span className="specs-card-objective">
                        {card.objective}
                      </span>
                      <span className="specs-card-meta">
                        {t("specs.card.revision", { revision: card.revision })}{" "}
                        · {t("specs.card.units", { count: card.units.length })}
                      </span>
                      {card.summary && (
                        <span className="specs-card-summary">
                          {card.summary}
                        </span>
                      )}
                      <span className="specs-card-badges">
                        <Badge
                          tone={publicationTone(
                            card.publication?.status ?? null,
                          )}
                        >
                          {t(
                            publicationLabel(card.publication?.status ?? null),
                          )}
                        </Badge>
                        {card.publication?.driftStatus === "DRIFTED" && (
                          <Badge tone="danger">
                            {t("specs.publication.drifted")}
                          </Badge>
                        )}
                      </span>
                    </button>
                  ))}
                </div>
              </section>
            );
          })}
        </section>
      )}

      {selected && (
        <Panel>
          <div className="specs-detail-heading">
            <div>
              <h2>{selected.objective}</h2>
              <p className="muted">
                {t("specs.detail.identity", {
                  revision: selected.revision,
                  request: shortId(selected.requestId),
                })}
              </p>
            </div>
            <Badge tone={columnTone(selected.column)}>
              {t(COLUMN_LABEL[selected.column] ?? "specs.column.frozen")}
            </Badge>
          </div>

          {detailError && (
            <p className="ui-callout ui-callout--danger" role="alert">
              {detailError}
            </p>
          )}
          {actionError && (
            <p className="ui-callout ui-callout--danger" role="alert">
              {actionError}
            </p>
          )}
          {notice && (
            <p className="ui-callout ui-callout--success" role="status">
              {notice}
            </p>
          )}

          <dl className="specs-facts">
            <div>
              <dt>{t("specs.detail.specSha")}</dt>
              <dd>
                <code>{selected.specSha256?.slice(0, 16) ?? "—"}</code>
              </dd>
            </div>
            <div>
              <dt>{t("specs.detail.graphSha")}</dt>
              <dd>
                <code>{selected.graphSha256?.slice(0, 16) ?? "—"}</code>
              </dd>
            </div>
            <div>
              <dt>{t("specs.detail.frozenAt")}</dt>
              <dd>{selected.frozenAt ? formatDate(selected.frozenAt) : "—"}</dd>
            </div>
            <div>
              <dt>{t("specs.detail.publication")}</dt>
              <dd>
                {selected.publication
                  ? `${selected.publication.provider} · ${selected.publication.status} · ${selected.publication.driftStatus}`
                  : t("specs.publication.unpublished")}
              </dd>
            </div>
          </dl>

          {selected.publication?.lastError && (
            <p className="ui-callout ui-callout--danger" role="alert">
              {selected.publication.lastError}
            </p>
          )}

          <section className="specs-section">
            <h3>{t("specs.detail.units")}</h3>
            <ul className="specs-units">
              {selected.units.map((unit) => (
                <li key={unit.id}>
                  <div className="specs-unit-heading">
                    <code>{unit.id}</code>
                    <strong>{unit.title}</strong>
                  </div>
                  <p className="muted">{unit.objective}</p>
                  <p className="muted">
                    {t("specs.detail.dependsOn")}:{" "}
                    {unit.dependsOn.length ? unit.dependsOn.join(", ") : "—"}
                  </p>
                  <p className="muted">
                    {t("specs.detail.paths")}: {unit.paths.join(", ")}
                  </p>
                </li>
              ))}
            </ul>
          </section>

          <section className="specs-section">
            <h3>{t("specs.detail.traceability")}</h3>
            {detail?.frozen?.spec ? (
              <Traceability spec={detail.frozen.spec} />
            ) : (
              <p className="muted">
                {t("specs.detail.traceabilityUnavailable")}
              </p>
            )}
          </section>

          <section className="specs-section">
            <h3>{t("specs.detail.issues")}</h3>
            {selected.issues.length === 0 ? (
              <p className="muted">{t("specs.detail.noIssues")}</p>
            ) : (
              <ul className="specs-issues">
                {selected.issues.map((issue) => (
                  <li key={`${issue.specId}:${issue.unitId}`}>
                    <code>{issue.unitId}</code>
                    {issue.externalUrl ? (
                      <a
                        href={issue.externalUrl}
                        target="_blank"
                        rel="noreferrer"
                      >
                        #{issue.externalNumber}
                      </a>
                    ) : (
                      <span className="muted">
                        {t("specs.detail.localOnly")}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </section>

          <div className="specs-actions">
            {selected.publication?.provider === "GITHUB" && (
              <Button
                variant="secondary"
                busy={busy === "refresh"}
                disabled={busy !== null}
                onClick={() => void refresh()}
              >
                {t("specs.refresh.action")}
              </Button>
            )}
            {detail?.githubBound ? (
              <>
                <label className="specs-confirm">
                  <input
                    type="checkbox"
                    checked={confirmPublish}
                    onChange={(event) =>
                      setConfirmPublish(event.target.checked)
                    }
                  />
                  {t("specs.publish.confirm")}
                </label>
                <Button
                  busy={busy === "publish"}
                  disabled={busy !== null || !confirmPublish}
                  onClick={() => void publish(false)}
                >
                  {t("specs.publish.action")}
                </Button>
              </>
            ) : (
              <p className="muted">{t("specs.publish.githubUnavailable")}</p>
            )}
            <Button
              variant="secondary"
              busy={busy === "local"}
              disabled={busy !== null}
              onClick={() => void publish(true)}
            >
              {t("specs.publish.localAction")}
            </Button>
          </div>
          <p className="muted">{t("specs.detail.authorityNote")}</p>
        </Panel>
      )}
    </div>
  );
}
