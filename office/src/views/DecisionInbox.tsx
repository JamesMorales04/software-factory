import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ModelCallResolution } from "../components/office/ModelCallResolution";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import { CoreApiError, coreApi, type DecisionItem } from "../coreApi";
import { type MessageKey, type Translate, useI18n } from "../i18n";

const TARGET_LABEL_KEYS: Record<string, MessageKey> = {
  "campaign.authorization": "decisions.target.campaigns",
  "lead.ready": "decisions.target.lead",
  "quality.gate": "decisions.target.campaign",
  "run.failed": "decisions.target.campaign",
  "model.usage-unresolved": "decisions.target.runtime",
  "provider.unresolved-session": "decisions.target.runtime",
  "action.human-required": "decisions.target.action",
};

function targetLabel(item: DecisionItem, t: Translate): string {
  const key = TARGET_LABEL_KEYS[item.kind];
  return key ? t(key) : t("decisions.target.default");
}

function toneForDecision(severity: DecisionItem["severity"]) {
  if (severity === "critical") return "danger" as const;
  if (severity === "attention") return "attention" as const;
  return "info" as const;
}

function DecisionCard({
  item,
  onResolved,
}: {
  item: DecisionItem;
  onResolved: () => Promise<void>;
}) {
  const { t, formatDate } = useI18n();
  return (
    <Panel className="decision-item">
      <div className="decision-item-head">
        <Badge tone={toneForDecision(item.severity)}>{item.severity}</Badge>
        <h2>{item.title}</h2>
        <span className="muted">{formatDate(item.updated_at)}</span>
      </div>
      <p>{item.detail}</p>
      <div className="decision-item-actions">
        <Link
          className="ui-button ui-button--secondary ui-button--sm"
          to={item.target}
        >
          {targetLabel(item, t)}
        </Link>
        <span className="muted">
          {t("decisions.subject", { id: item.subject_id })}
        </span>
      </div>
      {item.action?.kind === "resolve_model_call" && (
        <ModelCallResolution
          modelCallId={item.action.model_call_id ?? item.subject_id}
          onResolved={onResolved}
        />
      )}
    </Panel>
  );
}

export function DecisionInbox() {
  const { t, formatDate } = useI18n();
  const [items, setItems] = useState<DecisionItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asOf, setAsOf] = useState("");

  const load = useCallback(async () => {
    try {
      const next = await coreApi.getDecisions();
      setItems(next.items);
      setAsOf(next.as_of);
      setError(null);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("decisions.error.load"),
      );
    }
  }, [t]);

  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), 5000);
    return () => clearInterval(timer);
  }, [load]);

  if (error && !items) {
    return (
      <ErrorState
        title={t("common.errorTitle")}
        message={error}
        action={
          <Button onClick={() => void load()}>{t("common.retry")}</Button>
        }
      />
    );
  }
  if (!items) {
    return <LoadingState label={t("decisions.loading")} />;
  }

  const actionable = items.filter((item) => item.actionable);
  const awareness = items.filter((item) => !item.actionable);

  return (
    <div className="decision-workspace">
      <PageHeader
        title={t("decisions.title")}
        description={t("decisions.description")}
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
          </>
        }
      />
      {error && <p className="error">{error}</p>}
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
              <DecisionCard key={item.id} item={item} onResolved={load} />
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
              <DecisionCard key={item.id} item={item} onResolved={load} />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
