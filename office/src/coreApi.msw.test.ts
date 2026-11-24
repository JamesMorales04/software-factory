import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { CoreApiError, coreApi, recordedPlanReview } from "./coreApi";

/**
 * Contract-shaped network tests use MSW at the HTTP boundary instead of
 * stubbing our own module. This keeps the request/response shape close to the
 * real Core while making loading, error and retry paths deterministic.
 */
const server = setupServer(
  http.get("*/core-api/health", () =>
    HttpResponse.json({ healthy: true, version: "contract-test" }),
  ),
);

beforeAll(() => server.listen());
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe("Core API network boundary", () => {
  it("decodes the health projection", async () => {
    await expect(coreApi.health()).resolves.toEqual({
      healthy: true,
      version: "contract-test",
    });
  });

  it("surfaces a bounded HTTP failure", async () => {
    server.use(
      http.get("*/core-api/health", () =>
        HttpResponse.json({ error: "unavailable" }, { status: 503 }),
      ),
    );
    await expect(coreApi.health()).rejects.toMatchObject({ status: 503 });
  });
});

/**
 * The shapes below mirror the real success and `review_recorded_error`
 * responses in `native/src/bin/core.rs` at candidate `fd04d83e`. Core records
 * the owner decision before it prepares the environment, so a later failure
 * still reports the recorded decision and the real blocker.
 */
const recordedReview = {
  id: "review-1",
  requestId: "request-1",
  planRevision: 3,
  planSha256: "b".repeat(64),
  status: "OWNER_APPROVED",
  principalId: "operator:owner",
  rationale: "Owner approved the plan.",
  points: 0,
  reviewedAt: "2026-11-15T18:00:00Z",
};

const recordedDecisionFailure = {
  error: "the frozen Lead route is unavailable: provider offline",
  code: "ENVIRONMENT_LEAD_MODEL_UNAVAILABLE",
  review: recordedReview,
  reviewRecorded: true,
  environmentBlocked: false,
  workflow: null,
};

const reviewInput = {
  planRevision: 3,
  planSha256: "b".repeat(64),
  decision: "APPROVE" as const,
  rationale: "Owner approved the plan.",
};

describe("plan-review decision carrier", () => {
  it("decodes reviewRecorded and environmentBlocked from a successful review", async () => {
    server.use(
      http.post("*/core-api/agentic/requests/:id/plan-review", () =>
        HttpResponse.json({
          review: recordedReview,
          reviewRecorded: true,
          environmentBlocked: true,
          workflow: null,
        }),
      ),
    );
    await expect(
      coreApi.reviewAgenticPlan("request-1", reviewInput),
    ).resolves.toMatchObject({
      reviewRecorded: true,
      environmentBlocked: true,
      review: { status: "OWNER_APPROVED" },
    });
  });

  it("preserves a 2xx review that Core did not confirm as recorded", async () => {
    server.use(
      http.post("*/core-api/agentic/requests/:id/plan-review", () =>
        HttpResponse.json({
          review: recordedReview,
          reviewRecorded: false,
          environmentBlocked: false,
          workflow: null,
        }),
      ),
    );
    await expect(
      coreApi.reviewAgenticPlan("request-1", reviewInput),
    ).resolves.toMatchObject({ reviewRecorded: false });
  });

  it("consumes the recorded decision from a later environment failure", async () => {
    server.use(
      http.post("*/core-api/agentic/requests/:id/plan-review", () =>
        HttpResponse.json(recordedDecisionFailure, { status: 503 }),
      ),
    );
    const error = await coreApi
      .reviewAgenticPlan("request-1", reviewInput)
      .catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(CoreApiError);
    expect(error).toMatchObject({
      status: 503,
      code: "ENVIRONMENT_LEAD_MODEL_UNAVAILABLE",
    });
    expect(recordedPlanReview(error)).toEqual({
      reviewRecorded: true,
      environmentBlocked: false,
      reviewRevision: 3,
      message: "the frozen Lead route is unavailable: provider offline",
      code: "ENVIRONMENT_LEAD_MODEL_UNAVAILABLE",
    });
  });

  it("does not invent a recorded decision for an unrecorded failure", async () => {
    server.use(
      http.post("*/core-api/agentic/requests/:id/plan-review", () =>
        HttpResponse.json(
          {
            error: "a plan review requires a rationale",
            code: "RATIONALE_REQUIRED",
          },
          { status: 400 },
        ),
      ),
    );
    const error = await coreApi
      .reviewAgenticPlan("request-1", reviewInput)
      .catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(CoreApiError);
    expect(recordedPlanReview(error)).toBeNull();
  });
});
