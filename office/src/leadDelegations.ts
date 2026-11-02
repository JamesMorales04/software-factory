import type { DelegationProjection, StandingDelegationV2 } from "./coreApi";

export type DelegationProjectionStatus =
  | "ACTIVE"
  | "REVOKED"
  | "EXPIRED"
  | "UNKNOWN"
  | "HISTORICAL";

/** Classifies the Core-projected grant without treating malformed expiry as authority. */
export function delegationProjectionStatus(
  projection: DelegationProjection,
  nowMs = Date.now(),
): DelegationProjectionStatus {
  if (projection.schemaVersion === 1) return "HISTORICAL";
  const { delegation } = projection;
  if (delegation.revokedAt) return "REVOKED";
  const expiresAtMs = Date.parse(delegation.expiresAt);
  if (!Number.isFinite(expiresAtMs)) return "UNKNOWN";
  return expiresAtMs <= nowMs ? "EXPIRED" : "ACTIVE";
}

/** V1 list entries are historical evidence and cannot authorize a new Lead request. */
export function selectableLeadDelegations(
  projections: DelegationProjection[],
  projectId: string,
  nowMs = Date.now(),
): StandingDelegationV2[] {
  return projections.flatMap((projection) => {
    if (projection.schemaVersion !== 2) return [];
    const { delegation } = projection;
    if (
      delegation.projectId !== projectId ||
      delegationProjectionStatus(projection, nowMs) !== "ACTIVE"
    )
      return [];
    return [delegation];
  });
}
