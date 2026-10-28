import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import QRCode from "qrcode";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { AccessDevices } from "./AccessDevices";

const mocks = vi.hoisted(() => ({
  getAccessOverview: vi.fn(),
  listDeviceSessions: vi.fn(),
  createDeviceChallenge: vi.fn(),
  approveDevice: vi.fn(),
  rejectDevice: vi.fn(),
  revokeDevice: vi.fn(),
  getSecurityAudit: vi.fn(),
  cancelDeviceChallenge: vi.fn(),
  logoutAllDevices: vi.fn(),
  logoutCurrentDevice: vi.fn(),
}));

vi.mock("qrcode", () => ({
  default: { toCanvas: vi.fn().mockResolvedValue(undefined) },
}));

vi.mock("../coreApi", () => ({
  coreApi: mocks,
  logoutAllDevices: mocks.logoutAllDevices,
  logoutCurrentDevice: mocks.logoutCurrentDevice,
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
    localStorage.clear();
    document.documentElement.lang = "en";
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
    mocks.getSecurityAudit.mockReset().mockResolvedValue({ entries: [] });
    mocks.cancelDeviceChallenge.mockReset().mockResolvedValue({
      id: "challenge-public-id",
      status: "CANCELLED",
    });
    mocks.logoutCurrentDevice.mockReset().mockResolvedValue(undefined);
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

  it("renders the access workspace and pairing controls in Spanish", async () => {
    const user = userEvent.setup();
    render(
      <I18nProvider initialLocale="es">
        <AccessDevices />
      </I18nProvider>,
    );

    expect(
      await screen.findByRole("heading", { name: "Acceso local" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Dispositivos inscritos" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Emparejar dispositivo" }),
    ).toBeTruthy();
    expect(screen.getByText(/no se guarda en este navegador/i)).toBeTruthy();
    expect(document.documentElement.lang).toBe("es");

    await user.click(
      screen.getByRole("button", { name: "Emparejar dispositivo" }),
    );
    expect(
      await screen.findByRole("heading", {
        name: "Escanea con la cámara del teléfono",
      }),
    ).toBeTruthy();
    expect(screen.getByText("Código de verificación")).toBeTruthy();
    expect(
      screen.getByLabelText(
        "Código QR de emparejamiento de dispositivo de un solo uso",
      ),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", {
        name: "Cancelar código de emparejamiento",
      }),
    ).toBeTruthy();
  });

  it("shows an explicit recovery error when Core access state cannot load", async () => {
    mocks.getAccessOverview.mockRejectedValue(new Error("offline"));
    mocks.listDeviceSessions.mockRejectedValue(new Error("offline"));
    mocks.getSecurityAudit.mockRejectedValue(new Error("offline"));
    render(<AccessDevices />);
    expect(await screen.findByText("Something went wrong")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain(
      "Access status is unavailable",
    );
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
  });

  it("marks the current device and renders recent security activity", async () => {
    mocks.listDeviceSessions.mockResolvedValue({
      devices: [
        {
          id: "desktop-1",
          name: "Desktop",
          status: "CONNECTED",
          current: true,
          createdAt: "2026-10-27T21:00:00Z",
          lastActivityAt: "2026-10-27T21:00:00Z",
          expiresAt: "2026-11-27T22:00:00Z",
        },
      ],
    });
    mocks.getSecurityAudit.mockResolvedValue({
      entries: [
        {
          action: "created",
          actor: "owner-bootstrap",
          deviceName: "Desktop",
          occurredAt: "2026-10-27T21:00:00Z",
        },
        {
          action: "approved",
          actor: "Desktop",
          deviceName: "Phone",
          occurredAt: "2026-10-27T21:05:00Z",
        },
      ],
    });
    render(<AccessDevices />);
    expect(await screen.findByText("CURRENT")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Sign out" })).toBeTruthy();
    expect(screen.getByText("Session created")).toBeTruthy();
    expect(screen.getByText("Pairing approved")).toBeTruthy();
    expect(screen.getByText("by owner-bootstrap")).toBeTruthy();
  });

  it("cancels an unclaimed pairing code instead of leaving it reusable", async () => {
    const user = userEvent.setup();
    render(<AccessDevices />);
    await user.click(
      await screen.findByRole("button", { name: "Pair device" }),
    );
    await user.click(
      await screen.findByRole("button", { name: "Cancel pairing code" }),
    );
    await waitFor(() =>
      expect(mocks.cancelDeviceChallenge).toHaveBeenCalledWith(
        "challenge-public-id",
      ),
    );
    expect(screen.queryByText("Cancel pairing code")).toBeNull();
  });
});
