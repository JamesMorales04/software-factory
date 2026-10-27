import { AlertTriangle, Inbox, LoaderCircle } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "./cn";

export function Panel({
  children,
  className,
  as: Component = "section",
  "data-testid": testId,
}: {
  children: ReactNode;
  className?: string;
  as?: "section" | "div" | "aside";
  "data-testid"?: string;
}) {
  return (
    <Component className={cn("ui-panel", className)} data-testid={testId}>
      {children}
    </Component>
  );
}

export function PageHeader({
  title,
  description,
  actions,
  meta,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  meta?: ReactNode;
}) {
  return (
    <header className="ui-page-header">
      <div className="ui-page-header-copy">
        <h1 className="ui-page-title">{title}</h1>
        {description && <p className="ui-page-description">{description}</p>}
        {meta && <div className="ui-page-meta">{meta}</div>}
      </div>
      {actions && <div className="ui-page-actions">{actions}</div>}
    </header>
  );
}

export function EmptyState({
  title,
  description,
  action,
}: {
  title: ReactNode;
  description?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="ui-empty">
      <Inbox size={22} aria-hidden="true" />
      <h2>{title}</h2>
      {description && <p>{description}</p>}
      {action && <div className="ui-empty-action">{action}</div>}
    </div>
  );
}

export function ErrorState({
  title = "Something went wrong",
  message,
  action,
}: {
  title?: ReactNode;
  message: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="ui-error-state" role="alert">
      <AlertTriangle size={22} aria-hidden="true" />
      <h2>{title}</h2>
      <p>{message}</p>
      {action && <div className="ui-empty-action">{action}</div>}
    </div>
  );
}

export function LoadingState({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="ui-loading" role="status" aria-live="polite">
      <LoaderCircle size={18} className="ui-spin" aria-hidden="true" />
      <span>{label}</span>
    </div>
  );
}
