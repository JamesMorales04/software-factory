import { beforeEach, describe, expect, it, vi } from "vitest";
import { bootstrapOwner, getAuthSession, logoutCurrentDevice } from "./coreApi";
import {
  initializeOffice,
  LEGACY_OPERATOR_TOKEN_KEY,
} from "./legacyCredentials";

function jsonResponse(body: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
    text: async () => JSON.stringify(body),
  } as Response;
}

function setCsrfCookie(value: string) {
  Object.defineProperty(document, "cookie", {
    configurable: true,
    get: () => value,
  });
}

const LEGACY_TOKEN = "legacy-permanent-operator-token-value";

describe("Office legacy credential upgrade", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    sessionStorage.clear();
    setCsrfCookie("factory_csrf=csrf-value");
  });

  it("removes a pre-session operator token before enrollment and never uses it", async () => {
    sessionStorage.setItem(LEGACY_OPERATOR_TOKEN_KEY, LEGACY_TOKEN);
    sessionStorage.setItem("unrelated-office-key", "keep-me");

    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(async (input) => {
        const url = String(input);
        if (url.endsWith("/auth/bootstrap")) {
          return jsonResponse({
            authenticated: true,
            device: { id: "device-1", name: "Workstation" },
          });
        }
        if (url.endsWith("/auth/session")) {
          return jsonResponse({
            authenticated: true,
            device: { id: "device-1", name: "Workstation" },
          });
        }
        if (url.endsWith("/auth/logout")) {
          return jsonResponse(undefined, 204);
        }
        throw new Error(`unexpected request ${url}`);
      });

    // This is the first statement `main.tsx` executes when Office loads.
    initializeOffice();

    expect(sessionStorage.getItem(LEGACY_OPERATOR_TOKEN_KEY)).toBeNull();
    expect(sessionStorage.getItem("unrelated-office-key")).toBe("keep-me");

    await bootstrapOwner("one-time-confirmation", "Workstation");
    await getAuthSession();
    await logoutCurrentDevice();

    for (const [, request] of fetchMock.mock.calls) {
      const headers = new Headers(request?.headers);
      expect(headers.has("authorization")).toBe(false);
      expect(String(request?.body ?? "")).not.toContain(LEGACY_TOKEN);
      expect(String(request?.headers ?? "")).not.toContain(LEGACY_TOKEN);
    }
    expect(sessionStorage.getItem(LEGACY_OPERATOR_TOKEN_KEY)).toBeNull();
    expect(sessionStorage.length).toBe(1);
  });

  it("does not fail Office startup when session storage is unavailable", () => {
    const removeItem = vi
      .spyOn(Storage.prototype, "removeItem")
      .mockImplementation(() => {
        throw new Error("storage disabled");
      });

    expect(() => initializeOffice()).not.toThrow();
    removeItem.mockRestore();
  });
});
