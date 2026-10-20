import type { PaseoRecovery } from "../api";
import { formatTimestamp } from "../paseoView";

/**
 * Read-only S3-D recovery receipt. It reports what Core and the recovery
 * Worker actually observed. "confirmed" means the owned session/workspace
 * were verified archived/absent; it is not a claim of exactly-once external
 * side effects.
 */
export function RecoveryStatus({
  recovery,
}: {
  recovery: PaseoRecovery | undefined;
}) {
  if (!recovery) {
    return (
      <p className="muted recovery-summary">
        No recovery receipt yet. Core has not claimed this orphan for cleanup.
      </p>
    );
  }
  const lastObserved = recovery.lastObservedAt
    ? formatTimestamp(recovery.lastObservedAt)
    : "not observed yet";
  return (
    <section className={`recovery-summary recovery-${recovery.status}`}>
      <p>
        <span className={`pill recovery-${recovery.status}`}>
          cleanup {recovery.status}
        </span>{" "}
        <span className="muted">
          · {recovery.attempts} attempt{recovery.attempts === 1 ? "" : "s"} ·
          last observation {lastObserved}
        </span>
      </p>
      {recovery.claimOwner && recovery.status === "claimed" && (
        <p className="muted">Claimed by Worker {recovery.claimOwner}.</p>
      )}
      <p className="muted">{recoveryExplanation(recovery)}</p>
      {recovery.reason && (
        <p className={recovery.status === "blocked" ? "error" : "warning"}>
          {recovery.reason}
        </p>
      )}
    </section>
  );
}

function recoveryExplanation(recovery: PaseoRecovery): string {
  switch (recovery.status) {
    case "pending":
      return "Cleanup is queued but not yet claimed. The external session may still exist.";
    case "claimed":
      return "A Worker is observing or cleaning the owned external resources.";
    case "unconfirmed":
      return "External state is not settled yet. Automatic retries are bounded; no stop is claimed.";
    case "confirmed":
      return "Cleanup confirmed: the owned session was verified archived or absent and the workspace is not active. This is not proof of exactly-once external side effects.";
    case "absent":
      return "Already absent: two authoritative external checks found no owned session or workspace.";
    case "blocked":
      return "Automatic cleanup stopped at an identity, ownership or infrastructure boundary. Inspect the reason; no destructive action was taken for identity mismatches.";
    default:
      return "Recovery state is outside the documented vocabulary; treat it as unconfirmed.";
  }
}
