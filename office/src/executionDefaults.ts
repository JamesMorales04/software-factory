import type {
  ExecutionOperationLimits,
  ExecutionProgressPolicy,
} from "./coreApi";

/**
 * Technical defaults for a V2 Campaign policy. These are per-operation and
 * capacity bounds, not turn, attempt, session, token, cost or Campaign
 * deadline budgets.
 */
export const DEFAULT_OPERATION_LIMITS: ExecutionOperationLimits = {
  modelRequestTimeoutSecs: 300,
  toolCallTimeoutSecs: 600,
  maxRequestBytes: 2 * 1024 * 1024,
  maxResponseBytes: 2 * 1024 * 1024,
  maxGenerationTokens: 8_192,
  maxInFlightOperations: 4,
};

export const DEFAULT_PROGRESS_POLICY: ExecutionProgressPolicy = {
  diagnoseAfterStagnantCheckpoints: 2,
  escalateAfterUnsuccessfulReplans: 1,
  observationTimeoutSecs: 300,
};
