import { type MessageKey, useI18n } from "../../i18n";
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
  "CURRENT",
  "REGISTERED",
  "AVAILABLE",
  "MERGED",
  "REVERTED",
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
  "STALE",
  "DISCONNECTED",
  "WAITING",
  "ENDPOINT_UNSUPPORTED",
  "INVALID_RESPONSE",
  "NEEDS_ATTENTION",
  "OPEN",
  "ACKNOWLEDGED",
  "REVERT_PENDING",
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
  "CREDENTIAL_REJECTED",
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
  "MERGING",
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

const LABEL_KEYS: Record<string, MessageKey> = {
  ACCEPTED: "status.accepted",
  ACTIVE: "status.active",
  AMBIGUOUS: "status.ambiguous",
  AUTHORIZED: "status.authorized",
  BLOCKED: "status.blocked",
  BLOCKED_AUTHORIZATION: "status.blockedAuthorization",
  CANCEL_REQUESTED: "status.cancelRequested",
  CANCELLED: "status.cancelled",
  CANDIDATE_READY: "status.candidateReady",
  CHANGES_REQUESTED: "status.changesRequested",
  CLEANED: "status.cleaned",
  COMPLETED: "status.completed",
  CURRENT: "status.current",
  DISABLED: "status.disabled",
  ENABLED: "status.enabled",
  FAILED: "status.failed",
  HEALTHY: "status.healthy",
  HUMAN_REQUIRED: "status.humanRequired",
  INTENT: "status.intent",
  LEASED: "status.leased",
  NOT_TESTED: "status.notTested",
  ORPHANED: "status.orphaned",
  PASS: "status.pass",
  PENDING: "status.pending",
  QUEUED: "status.queued",
  REPAIR_REQUIRED: "status.repairRequired",
  RESERVED: "status.reserved",
  RUNNING: "status.running",
  SETTLED: "status.settled",
  STALE: "status.stale",
  UNKNOWN: "status.unknown",
  UNMEASURED: "status.unmeasured",
  UNHEALTHY: "status.unhealthy",
};

export function statusLabel(
  status: string,
  t: (key: MessageKey) => string,
): string {
  const key = LABEL_KEYS[status.toUpperCase()];
  return key ? t(key) : status.replaceAll("_", " ").toLowerCase();
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
  const { t } = useI18n();
  return (
    <Badge tone={statusTone(status)} className={className}>
      {label ?? statusLabel(status, t)}
    </Badge>
  );
}
