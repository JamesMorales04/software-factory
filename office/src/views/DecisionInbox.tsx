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
import { formatTimestamp } from "../format";

function targetLabel(item: DecisionItem): string {
  if (item.kind === "campaign.authorization") return "Open Campaigns";
  if (item.kind === "lead.ready") return "Open Lead";
  if (item.kind === "quality.gate") return "Open Campaign";
  if (item.kind === "run.failed") return "Open Campaign";
  if (item.kind === "model.usage-unresolved") return "Open Runtime";
  if (item.kind === "provider.unresolved-session") return "Open Runtime";
  if (item.kind === "action.human-required") return "Open Action";
  return "Open target";
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
  return (
    <Panel className="decision-item">
      <div className="decision-item-head">
        <Badge tone={toneForDecision(item.severity)}>{item.severity}</Badge>
        <h2>{item.title}</h2>
        <span className="muted">{formatTimestamp(item.updated_at)}</span>
      </div>
      <p>{item.detail}</p>
      <div className="decision-item-actions">
        <Link
          className="ui-button ui-button--secondary ui-button--sm"
          to={item.target}
        >
          {targetLabel(item)}
        </Link>
        <span className="muted">Subject {item.subject_id}</span>
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
          : "Failed to load Core decisions.",
      );
    }
  }, []);

  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), 5000);
    return () => clearInterval(timer);
  }, [load]);

  if (error && !items) {
    return (
      <ErrorState
        message={error}
        action={<Button onClick={() => void load()}>Retry</Button>}
      />
    );
  }
  if (!items) {
    return <LoadingState label="Loading decisions…" />;
  }

  const actionable = items.filter((item) => item.actionable);
  const awareness = items.filter((item) => !item.actionable);

  return (
    <div>
      <PageHeader
        title="Decision Inbox"
        description="Core-backed work that needs an operator decision or awareness. Nothing here is inferred from provider prose."
        meta={
          <>
            <span>{actionable.length} actionable</span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>{awareness.length} for awareness</span>
            <span className="overview-meta-separator" aria-hidden="true">
              /
            </span>
            <span>as of {formatTimestamp(asOf)}</span>
          </>
        }
      />
      {error && <p className="error">{error}</p>}
      <section
        className="decision-section"
        aria-labelledby="decision-actionable"
      >
        <div className="section-heading">
          <h2 id="decision-actionable">Needs action</h2>
          <span className="muted">
            Only Core operations and links are shown
          </span>
        </div>
        {actionable.length === 0 ? (
          <EmptyState
            title="Nothing needs action"
            description="Approvals, Gate repairs and unresolved usage appear when Core records them."
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
          <h2 id="decision-awareness">For awareness</h2>
          <span className="muted">
            Monitoring only; no operator action is exposed
          </span>
        </div>
        {awareness.length === 0 ? (
          <EmptyState
            title="No unresolved background work"
            description="Active or orphaned provider sessions, failed Runs and human-required Actions appear here."
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
