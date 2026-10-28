import { useCallback, useEffect, useRef, useState } from "react";
import {
  DecisionCard,
  type DecisionRevalidation,
  type RevalidateDecision,
} from "../components/office/DecisionCard";
import { Button } from "../components/ui/Button";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
} from "../components/ui/Surface";
import { CoreApiError, coreApi, type DecisionItem } from "../coreApi";
import { useI18n } from "../i18n";

export function DecisionInbox() {
  const { t, formatDate } = useI18n();
  const [items, setItems] = useState<DecisionItem[] | null>(null);
  const [asOf, setAsOf] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [offline, setOffline] = useState(false);
  const [lastLoadedAt, setLastLoadedAt] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const hasLoaded = useRef(false);
  const noticeRef = useRef<HTMLDivElement>(null);

  const load = useCallback(
    async (_background = false) => {
      try {
        const next = await coreApi.getDecisions();
        hasLoaded.current = true;
        setItems(next.items);
        setAsOf(next.as_of);
        setLastLoadedAt(new Date().toISOString());
        setError(null);
        setConnectionError(null);
        setOffline(false);
      } catch (cause) {
        const message =
          cause instanceof CoreApiError
            ? cause.status === 401
              ? t("decisions.error.unauthorized")
              : cause.message
            : t("decisions.connectionError");
        setOffline(!(cause instanceof CoreApiError));
        if (hasLoaded.current) {
          setConnectionError(message);
        } else {
          setError(message);
        }
      }
    },
    [t],
  );

  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(true), 5000);
    return () => clearInterval(timer);
  }, [load]);

  useEffect(() => {
    const retry = () => void load(true);
    window.addEventListener("online", retry);
    return () => window.removeEventListener("online", retry);
  }, [load]);

  useEffect(() => {
    if (notice) noticeRef.current?.focus();
  }, [notice]);

  const revalidate = useCallback<RevalidateDecision>(
    async (id, revision): Promise<DecisionRevalidation> => {
      const next = await coreApi.getDecisions();
      setItems(next.items);
      setAsOf(next.as_of);
      setLastLoadedAt(new Date().toISOString());
      setConnectionError(null);
      setOffline(false);
      const current = next.items.find((item) => item.id === id);
      if (!current) return { status: "missing" };
      if (revision && current.revision !== revision) {
        return { status: "changed", item: current };
      }
      return { status: "current", item: current };
    },
    [],
  );

  const onMutated = useCallback(
    async (message: string) => {
      setNotice(message);
      await load(true);
    },
    [load],
  );

  const onStale = useCallback((message: string) => {
    setNotice(message);
  }, []);

  const onOffline = useCallback(() => {
    setOffline(true);
    setConnectionError(t("decisions.connectionError"));
  }, [t]);

  async function refresh() {
    setRefreshing(true);
    try {
      await load(false);
    } finally {
      setRefreshing(false);
    }
  }

  if (items === null) {
    if (error) {
      return (
        <ErrorState
          title={offline ? t("decisions.offlineTitle") : t("common.errorTitle")}
          message={error}
          action={
            <Button onClick={() => void refresh()}>{t("common.retry")}</Button>
          }
        />
      );
    }
    return <LoadingState label={t("decisions.loading")} />;
  }

  const actionable = items.filter((item) => item.actionable);
  const awareness = items.filter((item) => !item.actionable);

  return (
    <div className="decision-workspace">
      <PageHeader
        title={t("decisions.title")}
        description={t("decisions.description")}
        actions={
          <Button
            variant="secondary"
            onClick={() => void refresh()}
            disabled={refreshing}
          >
            {refreshing ? t("decisions.loading") : t("decisions.refresh")}
          </Button>
        }
        meta={
          <>
            <span>
              {t("decisions.actionableCount", { count: actionable.length })}
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>
              {t("decisions.awarenessCount", { count: awareness.length })}
            </span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>{t("decisions.asOf", { date: formatDate(asOf) })}</span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>{offline ? t("decisions.stale") : t("decisions.live")}</span>
            {lastLoadedAt && (
              <span className="muted">
                {t("decisions.lastLoaded", { date: formatDate(lastLoadedAt) })}
              </span>
            )}
          </>
        }
      />

      {connectionError && (
        <div
          className={`ui-callout ${
            offline ? "ui-callout--attention" : "ui-callout--danger"
          }`}
          role="status"
        >
          <strong>
            {offline ? t("decisions.offlineTitle") : t("decisions.error.load")}
          </strong>{" "}
          <span>{connectionError}</span>
          {lastLoadedAt && (
            <span>
              {" "}
              {t("decisions.lastLoaded", {
                date: formatDate(lastLoadedAt),
              })}
            </span>
          )}
          {offline && <p>{t("decisions.offlineDetail")}</p>}
          <Button size="sm" onClick={() => void refresh()}>
            {t("common.retry")}
          </Button>
        </div>
      )}

      <div
        ref={noticeRef}
        className={
          notice
            ? "ui-callout ui-callout--info decision-status"
            : "decision-status decision-status--empty"
        }
        role="status"
        aria-live="polite"
        tabIndex={-1}
      >
        {notice}
      </div>

      <section
        className="decision-section"
        aria-labelledby="decision-actionable"
      >
        <div className="section-heading">
          <h2 id="decision-actionable">{t("decisions.needsAction")}</h2>
          <span className="muted">{t("decisions.needsActionHint")}</span>
        </div>
        {actionable.length === 0 ? (
          <EmptyState
            title={t("decisions.emptyActionableTitle")}
            description={t("decisions.emptyActionableDescription")}
          />
        ) : (
          <div className="decision-list">
            {actionable.map((item) => (
              <DecisionCard
                key={item.id}
                item={item}
                offline={offline}
                revalidate={revalidate}
                onMutated={onMutated}
                onStale={onStale}
                onOffline={onOffline}
              />
            ))}
          </div>
        )}
      </section>

      <section
        className="decision-section"
        aria-labelledby="decision-awareness"
      >
        <div className="section-heading">
          <h2 id="decision-awareness">{t("decisions.awareness")}</h2>
          <span className="muted">{t("decisions.awarenessHint")}</span>
        </div>
        {awareness.length === 0 ? (
          <EmptyState
            title={t("decisions.emptyAwarenessTitle")}
            description={t("decisions.emptyAwarenessDescription")}
          />
        ) : (
          <div className="decision-list">
            {awareness.map((item) => (
              <DecisionCard
                key={item.id}
                item={item}
                offline={offline}
                revalidate={revalidate}
                onMutated={onMutated}
                onStale={onStale}
                onOffline={onOffline}
              />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
