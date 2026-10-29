import { describe, expect, it } from "vitest";
import type { DelegationProjection } from "./coreApi";
import { selectableLeadDelegations } from "./leadDelegations";

const v1Historical = {
  schemaVersion: 1,
  historical: true,
  usage: null,
  delegation: {
    id: "legacy-grant",
    principalId: "operator",
    auditId: "legacy-audit",
    projectId: "project-a",
    revision: 1,
    contentHash: "legacy-hash",
    objectiveClasses: ["feature"],
    allowedPaths: ["src"],
    capabilities: ["repository-edit"],
    riskClasses: ["R0"],
    autonomousStart: false,
    autonomousRiskClasses: [],
    route: { provider: "opencode-go", model: "model", mode: "build" },
    maxAttempts: 1,
    maxTimeoutSecs: 60,
    maxRepairCycles: 0,
    maxWallClockSecs: 60,
    maxParallel: 1,
    maxSessions: 1,
    maxRequests: 1,
    maxCumulativeSessions: 1,
    maxCumulativeAttempts: 1,
    expiresAt: "2099-01-01T00:00:00Z",
  },
} satisfies DelegationProjection;

const v2Current = {
  schemaVersion: 2,
  usage: null,
  delegation: {
    id: "current-grant",
    principalId: "operator",
    auditId: "policy-audit",
    projectId: "project-a",
    revision: 1,
    contentHash: "current-hash",
    objectiveClasses: ["feature"],
    allowedPaths: ["src"],
    capabilities: ["repository-edit"],
    riskClasses: ["R0"],
    autonomousStart: false,
    autonomousRiskClasses: [],
    route: { provider: "opencode-go", model: "model", mode: "build" },
    maxTimeoutSecs: 60,
    maxParallel: 1,
    operations: {
      modelRequestTimeoutSecs: 60,
      toolCallTimeoutSecs: 60,
      maxRequestBytes: 1024,
      maxResponseBytes: 1024,
      maxGenerationTokens: 128,
      maxInFlightOperations: 1,
    },
    progress: {
      diagnoseAfterStagnantCheckpoints: 1,
      escalateAfterUnsuccessfulReplans: 1,
      observationTimeoutSecs: 60,
    },
    expiresAt: "2099-01-01T00:00:00Z",
  },
} satisfies DelegationProjection;

describe("selectableLeadDelegations", () => {
  it("never treats a historical V1 grant as Lead authorization", () => {
    expect(selectableLeadDelegations([v1Historical], "project-a")).toEqual([]);
  });

  it("selects only active V2 grants scoped to the chosen Project", () => {
    const revoked = {
      ...v2Current,
      delegation: { ...v2Current.delegation, id: "revoked", revokedAt: "now" },
    } satisfies DelegationProjection;
    const otherProject = {
      ...v2Current,
      delegation: {
        ...v2Current.delegation,
        id: "other",
        projectId: "project-b",
      },
    } satisfies DelegationProjection;

    expect(
      selectableLeadDelegations(
        [v1Historical, v2Current, revoked, otherProject],
        "project-a",
      ).map((grant) => grant.id),
    ).toEqual(["current-grant"]);
  });
});
