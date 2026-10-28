import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import QRCode from "qrcode";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AccessDevices } from "./AccessDevices";

const mocks = vi.hoisted(() => ({
  getAccessOverview: vi.fn(),
  listDeviceSessions: vi.fn(),
  createDeviceChallenge: vi.fn(),
  approveDevice: vi.fn(),
  rejectDevice: vi.fn(),
  revokeDevice: vi.fn(),
  logoutAllDevices: vi.fn(),
}));

vi.mock("qrcode", () => ({
  default: { toCanvas: vi.fn().mockResolvedValue(undefined) },
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  logoutAllDevices: mocks.logoutAllDevices,
  CoreApiError: class CoreApiError extends Error {},
}));

const access = {
  local: { status: "READY" },
  tailscale: {
    enabled: true,
    status: "UNKNOWN",
    httpsEndpoint: "https://factory.example-tailnet.ts.net",
    error: null,
  },
};

describe("Access & Devices", () => {
  beforeEach(() => {
    mocks.getAccessOverview.mockReset().mockResolvedValue(access);
    mocks.listDeviceSessions.mockReset().mockResolvedValue({ devices: [] });
    mocks.createDeviceChallenge.mockReset().mockResolvedValue({
      challengeId: "challenge-public-id",
      verificationCode: "483921",
      expiresAt: "2026-10-27T22:00:00Z",
    });
    mocks.approveDevice.mockReset().mockResolvedValue({ id: "pending-1" });
    mocks.rejectDevice.mockReset().mockResolvedValue({ id: "pending-1" });
    mocks.revokeDevice.mockReset().mockResolvedValue(undefined);
  });

  it("shows configured remote access without claiming unknown connectivity", async () => {
    render(<AccessDevices />);
    expect(
      await screen.findByText("https://factory.example-tailnet.ts.net"),
    ).toBeTruthy();
    expect(screen.getByText("UNKNOWN")).toBeTruthy();
    expect(screen.getByText(/Do not treat tailnet membership/)).toBeTruthy();
    expect(
      screen.getByText(/public Certificate Transparency logs/),
    ).toBeTruthy();
    expect(screen.getByText(/Funnel remains disabled/)).toBeTruthy();
  });

  it.each(["CONNECTED", "DISCONNECTED", "UNKNOWN", "DISABLED"])(
    "renders the Core-reported Tailscale state %s",
    async (status) => {
      mocks.getAccessOverview.mockResolvedValue({
        local: { status: "AVAILABLE" },
        tailscale: {
          enabled: status !== "DISABLED",
          status,
          httpsEndpoint:
            status === "DISABLED" ? null : access.tailscale.httpsEndpoint,
          error: status === "DISCONNECTED" ? "health probe failed" : null,
        },
      });
      render(<AccessDevices />);
      expect((await screen.findAllByText(status)).length).toBeGreaterThan(0);
      if (status === "DISCONNECTED") {
        expect(screen.getByText(/health probe failed/)).toBeTruthy();
        expect(screen.getByText(/Core cannot reach/)).toBeTruthy();
      }
      if (status === "DISABLED") {
        expect(screen.getByText(/Remote access is disabled/)).toBeTruthy();
      }
    },
  );

  it("creates a harmless QR challenge and requires code confirmation before approval", async () => {
    const user = userEvent.setup();
    mocks.listDeviceSessions.mockResolvedValue({
      devices: [
        {
          id: "pending-1",
          name: "Phone",
          status: "PENDING",
          verificationCode: "483921",
          createdAt: "2026-10-27T21:00:00Z",
          lastActivityAt: "2026-10-27T21:00:00Z",
          expiresAt: "2026-10-27T22:00:00Z",
        },
      ],
    });
    render(<AccessDevices />);

    await user.click(
      await screen.findByRole("button", { name: "Pair device" }),
    );
    expect(await screen.findAllByText("483921")).toHaveLength(2);
    expect(
      screen.getByLabelText("One-time device pairing QR code"),
    ).toBeTruthy();
    expect(screen.queryByText("challenge-public-id")).toBeNull();
    await waitFor(() => expect(QRCode.toCanvas).toHaveBeenCalled());
    const qrUrl = vi.mocked(QRCode.toCanvas).mock.calls[0]?.[1];
    expect(qrUrl).toContain(
      "https://factory.example-tailnet.ts.net/#/pair?challengeId=",
    );
    expect(qrUrl).not.toContain("claimToken");

    const approve = screen.getByRole("button", { name: "Approve device" });
    expect(approve.hasAttribute("disabled")).toBe(true);
    await user.type(
      screen.getByRole("textbox", {
        name: "Confirm the verification code shown on the phone",
      }),
      "483921",
    );
    await user.click(approve);
    await waitFor(() =>
      expect(mocks.approveDevice).toHaveBeenCalledWith("pending-1", "483921"),
    );
  });

  it("shows an explicit recovery error when Core access state cannot load", async () => {
    mocks.getAccessOverview.mockRejectedValue(new Error("offline"));
    mocks.listDeviceSessions.mockRejectedValue(new Error("offline"));
    render(<AccessDevices />);
    expect(await screen.findByText("Something went wrong")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain(
      "Access status is unavailable",
    );
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
  });
});
