import type { DelegationProjection, StandingDelegationV2 } from "./coreApi";

/** V1 list entries are historical evidence and cannot authorize a new Lead request. */
export function selectableLeadDelegations(
  projections: DelegationProjection[],
  projectId: string,
): StandingDelegationV2[] {
  return projections.flatMap((projection) => {
    if (projection.schemaVersion !== 2) return [];
    const { delegation } = projection;
    if (delegation.projectId !== projectId || delegation.revokedAt) return [];
    return [delegation];
  });
}
