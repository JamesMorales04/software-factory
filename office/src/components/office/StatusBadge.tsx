import { Badge, type BadgeTone } from "../ui/Badge";

const SUCCESS = new Set([
  "ACCEPTED",
  "PASS",
  "PASSED",
  "COMPLETED",
  "CLEANED",
  "SUCCEEDED",
  "SETTLED",
  "VERIFIED",
  "HEALTHY",
  "ENABLED",
  "AUTHENTICATED",
  "CONFIGURED",
]);

const ATTENTION = new Set([
  "REPAIR_REQUIRED",
  "BLOCKED_AUTHORIZATION",
  "HUMAN_REQUIRED",
  "CANCEL_REQUESTED",
  "UNKNOWN",
  "UNMEASURED",
  "ORPHANED",
  "NOT_TESTED",
  "PENDING",
  "DENIED",
  "UNSUPPORTED",
  "DETERMINISTIC_ONLY",
  "BLOCKED",
]);

const DANGER = new Set([
  "FAILED",
  "FAIL",
  "REJECTED",
  "BLOCKED",
  "ERROR",
  "CANCELLED",
  "UNHEALTHY",
  "UNREACHABLE",
  "CHANGES_REQUESTED",
]);

const INFO = new Set([
  "ACTIVE",
  "RUNNING",
  "AUTHORIZED",
  "INTENT",
  "READY",
  "QUEUED",
  "RESERVED",
  "SUPPORTED_TRUSTED_LOCAL",
  "DEFAULT",
]);

export function statusTone(status: string): BadgeTone {
  const value = status.toUpperCase();
  if (SUCCESS.has(value)) return "success";
  if (ATTENTION.has(value)) return "attention";
  if (DANGER.has(value)) return "danger";
  if (INFO.has(value)) return "info";
  if (value === "EXECUTED") return "success";
  if (value === "MOCKED") return "attention";
  if (value === "UNKNOWN") return "attention";
  return "neutral";
}

export function StatusBadge({
  status,
  className,
  label,
}: {
  status: string;
  className?: string;
  label?: string;
}) {
  return (
    <Badge tone={statusTone(status)} className={className}>
      {label ?? status}
    </Badge>
  );
}
