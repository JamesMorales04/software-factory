import type { ReactNode } from "react";
import { cn } from "./cn";

export type BadgeTone =
  | "neutral"
  | "accent"
  | "success"
  | "attention"
  | "danger"
  | "info"
  | "muted";

export function Badge({
  tone = "neutral",
  children,
  className,
}: {
  tone?: BadgeTone;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span className={cn("ui-badge", `ui-badge--${tone}`, className)}>
      {children}
    </span>
  );
}
