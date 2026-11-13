import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { coreApi } from "./coreApi";

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
