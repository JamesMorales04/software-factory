import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  bootstrapOwner,
  getAuthSession,
  logoutCurrentDevice,
} from "./coreApi";

function jsonResponse(body: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
    text: async () => JSON.stringify(body),
  } as Response;
}

describe("Office device-session API", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    document.cookie = "factory_csrf=; Max-Age=0; path=/";
  });

  it("loads the cookie session without sending a bearer credential", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ authenticated: true, device: { id: "device-1" } }),
    );

    await getAuthSession();

    expect(fetchMock).toHaveBeenCalledWith(
      "/core-api/auth/session",
      expect.objectContaining({ credentials: "same-origin" }),
    );
    const [, request] = fetchMock.mock.calls[0] ?? [];
    expect(new Headers(request?.headers).has("authorization")).toBe(false);
  });

  it("submits owner confirmation in the request body and uses the CSRF cookie", async () => {
    document.cookie = "factory_csrf=csrf-value; path=/; SameSite=Strict";
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ authenticated: true, device: { id: "device-1" } }),
    );

    await bootstrapOwner("one-time-confirmation", "Workstation");

    const [url, request] = fetchMock.mock.calls[0] ?? [];
    expect(url).toBe("/core-api/auth/bootstrap");
    expect(request?.credentials).toBe("same-origin");
    expect(request?.body).toBe(
      JSON.stringify({ code: "one-time-confirmation", deviceName: "Workstation" }),
    );
    expect(String(request?.body)).not.toMatch(/https?:\/\//);
    expect(new Headers(request?.headers).has("authorization")).toBe(false);
  });

  it("sends CSRF on logout and does not persist a session token", async () => {
    document.cookie = "factory_csrf=csrf-value; path=/; SameSite=Strict";
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse(undefined, 204),
    );

    await logoutCurrentDevice();

    const [url, request] = fetchMock.mock.calls[0] ?? [];
    expect(url).toBe("/core-api/auth/logout");
    expect(new Headers(request?.headers).get("x-factory-csrf")).toBe("csrf-value");
    expect(new Headers(request?.headers).has("authorization")).toBe(false);
    expect(sessionStorage.length).toBe(0);
  });
});
