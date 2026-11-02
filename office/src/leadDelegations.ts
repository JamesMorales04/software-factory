import type { DelegationProjection, StandingDelegationV2 } from "./coreApi";

/** A malformed or elapsed expiry is never selectable authority. */
export function isDelegationExpired(
  delegation: Pick<StandingDelegationV2, "expiresAt">,
  nowMs = Date.now(),
): boolean {
  const expiresAt = Date.parse(delegation.expiresAt);
  return !Number.isFinite(expiresAt) || expiresAt <= nowMs;
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
      delegation.revokedAt ||
      isDelegationExpired(delegation, nowMs)
    ) {
      return [];
    }
    return [delegation];
  });
}
