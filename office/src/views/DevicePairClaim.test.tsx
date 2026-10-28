import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DevicePairClaim } from "./DevicePairClaim";

const mocks = vi.hoisted(() => ({
  claimDevice: vi.fn(),
  pollDeviceClaim: vi.fn(),
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  CoreApiError: class CoreApiError extends Error {},
}));

describe("phone device pairing", () => {
  beforeEach(() => {
    mocks.claimDevice.mockReset().mockResolvedValue({
      deviceId: "phone-1",
      verificationCode: "483921",
      status: "PENDING",
      expiresAt: "2026-10-27T22:00:00Z",
      pollToken: "in-memory-poll-secret",
    });
    mocks.pollDeviceClaim.mockReset().mockResolvedValue({
      authenticated: false,
      status: "PENDING",
    });
    window.history.replaceState(
      null,
      "",
      "/#/pair?challengeId=public-challenge",
    );
  });

  it("strips the challenge identifier from history before requesting approval", async () => {
    const user = userEvent.setup();
    const view = render(
      <MemoryRouter initialEntries={["/pair?challengeId=public-challenge"]}>
        <Routes>
          <Route path="/pair" element={<DevicePairClaim />} />
        </Routes>
      </MemoryRouter>,
    );

    await waitFor(() => expect(window.location.hash).toBe("#/pair"));
    await user.type(
      screen.getByRole("textbox", { name: "Device name" }),
      "My phone",
    );
    await user.click(
      screen.getByRole("button", { name: "Request owner approval" }),
    );

    expect(await screen.findByText("483921")).toBeTruthy();
    expect(mocks.claimDevice).toHaveBeenCalledWith(
      "public-challenge",
      "My phone",
    );
    expect(window.location.hash).toBe("#/pair");
    view.unmount();
  });

  it("shows a rejected request without creating an Office session", async () => {
    const user = userEvent.setup();
    mocks.claimDevice.mockResolvedValue({
      deviceId: "phone-1",
      verificationCode: "483921",
      status: "REJECTED",
      expiresAt: "2026-10-27T22:00:00Z",
      pollToken: "poll",
    });
    render(
      <MemoryRouter initialEntries={["/pair?challengeId=public-challenge"]}>
        <Routes>
          <Route path="/pair" element={<DevicePairClaim />} />
        </Routes>
      </MemoryRouter>,
    );
    await user.click(
      screen.getByRole("button", { name: "Request owner approval" }),
    );
    expect(
      await screen.findByText(/owner rejected this device request/i),
    ).toBeTruthy();
    expect(mocks.pollDeviceClaim).not.toHaveBeenCalled();
  });
});
