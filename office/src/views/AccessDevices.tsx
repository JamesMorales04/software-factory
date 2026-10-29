import QRCode from "qrcode";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "../components/ui/Button";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  Panel,
} from "../components/ui/Surface";
import {
  type AccessOverview,
  CoreApiError,
  coreApi,
  type DeviceChallenge,
  type DeviceSessionView,
  logoutAllDevices,
  logoutCurrentDevice,
  type SecurityAuditEntry,
} from "../coreApi";
import { formatTimestamp } from "../format";

const AUDIT_LABELS: Record<string, string> = {
  created: "Session created",
  rotated: "Session rotated",
  revoked: "Session revoked",
  revoked_all: "All sessions revoked",
  claimed: "Pairing claimed",
  approved: "Pairing approved",
  rejected: "Pairing rejected",
  cancelled: "Pairing cancelled",
  consumed: "Pairing completed",
  revocation_requested: "Revocation requested",
};

function auditLabel(action: string): string {
  return AUDIT_LABELS[action] ?? action.replaceAll("_", " ");
}

function challengeUrl(
  access: AccessOverview,
  challenge: DeviceChallenge,
): string {
  const configured = access.tailscale.httpsEndpoint;
  const origin = configured
    ? new URL(configured).origin
    : window.location.origin;
  const fragment = new URLSearchParams({ challengeId: challenge.challengeId });
  return `${origin}/#/pair?${fragment.toString()}`;
}

function PairingQr({ value }: { value: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!canvas.current) return;
    let active = true;
    void QRCode.toCanvas(canvas.current, value, {
      width: 256,
      margin: 2,
      errorCorrectionLevel: "L",
      color: { dark: "#07111f", light: "#ffffff" },
    }).then(
      () => active && setError(null),
      () => active && setError("The pairing code could not be rendered."),
    );
    return () => {
      active = false;
    };
  }, [value]);

  return (
    <div className="access-qr">
      <canvas ref={canvas} aria-label="One-time device pairing QR code" />
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

function DeviceRow({
  device,
  onChanged,
}: {
  device: DeviceSessionView;
  onChanged: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmedCode, setConfirmedCode] = useState("");

  async function mutate(operation: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await operation();
      await onChanged();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError ? cause.message : "Device action failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <li className="access-device">
      <div className="access-device-main">
        <div className="access-device-title">
          <strong>{device.name}</strong>
          {device.current && (
            <span className="access-status access-status--current">
              CURRENT
            </span>
          )}
          <span
            className={`access-status access-status--${device.status.toLowerCase()}`}
          >
            {device.status}
          </span>
        </div>
        {device.status === "PENDING" ? (
          <>
            <p className="muted">
              Verify code{" "}
              <code className="access-code">
                {device.verificationCode ?? "Unavailable"}
              </code>{" "}
              on both devices before approval.
            </p>
            <label className="ui-field access-code-confirm">
              <span className="ui-field-label">
                Enter the code shown on the phone
              </span>
              <input
                className="ui-input"
                inputMode="numeric"
                autoComplete="one-time-code"
                value={confirmedCode}
                onChange={(event) => setConfirmedCode(event.target.value)}
                aria-label="Confirm the verification code shown on the phone"
              />
            </label>
          </>
        ) : device.status === "APPROVED" ? (
          <p className="muted">
            Approved. Waiting for the phone to finish the one-time exchange; the
            approval expires if it is not picked up.
          </p>
        ) : (
          <p className="muted">
            Last activity{" "}
            {device.lastActivityAt
              ? formatTimestamp(device.lastActivityAt)
              : "not recorded"}{" "}
            · expires {formatTimestamp(device.expiresAt)}
          </p>
        )}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
      </div>
      <div className="access-device-actions">
        {device.status === "PENDING" ? (
          <>
            <Button
              size="sm"
              variant="primary"
              disabled={
                busy || confirmedCode.trim() !== (device.verificationCode ?? "")
              }
              onClick={() =>
                void mutate(() =>
                  coreApi.approveDevice(device.id, confirmedCode.trim()),
                )
              }
            >
              Approve device
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => void mutate(() => coreApi.rejectDevice(device.id))}
            >
              Reject
            </Button>
          </>
        ) : device.status === "APPROVED" ? (
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() =>
              void mutate(() => coreApi.cancelDeviceChallenge(device.id))
            }
          >
            Cancel approval
          </Button>
        ) : (
          <Button
            size="sm"
            variant="danger"
            disabled={busy}
            onClick={() => {
              if (device.current) {
                if (
                  window.confirm(
                    "Sign out this device? You will need to enroll it again.",
                  )
                ) {
                  void mutate(async () => {
                    await logoutCurrentDevice();
                    window.location.reload();
                  });
                }
                return;
              }
              if (window.confirm(`Revoke ${device.name}'s device session?`)) {
                void mutate(() => coreApi.revokeDevice(device.id));
              }
            }}
          >
            {device.current ? "Sign out" : "Revoke"}
          </Button>
        )}
      </div>
    </li>
  );
}

export function AccessDevices() {
  const [access, setAccess] = useState<AccessOverview | null>(null);
  const [devices, setDevices] = useState<DeviceSessionView[]>([]);
  const [challenge, setChallenge] = useState<DeviceChallenge | null>(null);
  const [audit, setAudit] = useState<SecurityAuditEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextAccess, nextDevices, nextAudit] = await Promise.all([
        coreApi.getAccessOverview(),
        coreApi.listDeviceSessions(),
        coreApi.getSecurityAudit(),
      ]);
      setAccess(nextAccess);
      setDevices(nextDevices.devices);
      setAudit(nextAudit.entries);
      setError(null);
      setLoading(false);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Access status is unavailable.",
      );
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const qrValue = useMemo(
    () => (access && challenge ? challengeUrl(access, challenge) : null),
    [access, challenge],
  );

  const sortedDevices = useMemo(() => {
    const rank = (device: DeviceSessionView) => {
      if (device.current) return 0;
      if (device.status === "PENDING") return 1;
      if (device.status === "CONNECTED") return 2;
      return 3;
    };
    return [...devices].sort((left, right) => rank(left) - rank(right));
  }, [devices]);

  async function pairDevice() {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      if (!access) await refresh();
      const next = await coreApi.createDeviceChallenge();
      setChallenge(next);
      setNotice(
        "Challenge created. It expires automatically and can be used once.",
      );
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Could not create a pairing challenge.",
      );
    } finally {
      setBusy(false);
    }
  }

  async function cancelChallenge() {
    if (!challenge) return;
    setBusy(true);
    setError(null);
    try {
      await coreApi.cancelDeviceChallenge(challenge.challengeId);
      setNotice("Pairing code cancelled. It cannot be claimed or approved.");
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Could not cancel the pairing code.",
      );
    } finally {
      setChallenge(null);
      setBusy(false);
    }
  }

  async function revokeAll() {
    if (
      !window.confirm(
        "Revoke every enrolled device session, including this device? You will need to enroll again.",
      )
    )
      return;
    setBusy(true);
    setError(null);
    try {
      await logoutAllDevices();
      window.location.reload();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Could not revoke device sessions.",
      );
      setBusy(false);
    }
  }

  if (loading)
    return <LoadingState label="Loading access and device sessions…" />;
  if (!access && error) {
    return (
      <ErrorState
        message={error}
        action={<Button onClick={() => void refresh()}>Retry</Button>}
      />
    );
  }
  if (!access)
    return (
      <EmptyState
        title="Access status unknown"
        description="Core did not return access information."
      />
    );

  return (
    <div className="access-workspace">
      {error && (
        <div className="ui-callout ui-callout--danger" role="alert">
          {error}
        </div>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}
      <Panel>
        <div className="section-heading">
          <div>
            <h2>Local access</h2>
            <p className="muted">Office access state from Core</p>
          </div>
          <div className="row">
            <span className="access-status">{access.local.status}</span>
            <Button size="sm" onClick={() => void refresh()}>
              Refresh status
            </Button>
          </div>
        </div>
        <p className="muted">
          Office uses an independent, revocable browser session. The operator
          recovery credential is not stored in this browser.
        </p>
      </Panel>

      <Panel>
        <div className="section-heading">
          <h2>Private remote access</h2>
          <span
            className={`access-status access-status--${access.tailscale.status.toLowerCase()}`}
          >
            {access.tailscale.status}
          </span>
        </div>
        {access.tailscale.status === "CONFIGURED" ? (
          <p className="muted">
            Private remote access is configured. Core cannot probe Tailscale
            health across the isolated socket-only boundary; open the private
            HTTPS endpoint to confirm the Serve path. Factory still requires its
            own enrolled device session.
          </p>
        ) : access.tailscale.status === "CONNECTED" ? (
          <p className="muted">
            Core can reach the private Office health endpoint. This confirms the
            current Serve path only; Factory still requires its own enrolled
            device session.
          </p>
        ) : access.tailscale.status === "DISCONNECTED" ? (
          <p className="muted">
            Core cannot reach the configured private Office health endpoint.
            Check the Tailscale service, Serve configuration, and private HTTPS
            endpoint in `docs/operations/compose-deployment.md`.
          </p>
        ) : access.tailscale.status === "UNKNOWN" ? (
          <p className="muted">
            Tailscale connectivity could not be verified. Check the service and
            private HTTPS endpoint in `docs/operations/compose-deployment.md`.
            Do not treat tailnet membership as a Factory session.
          </p>
        ) : (
          <p className="muted">
            Remote access is disabled. To enable it, follow the optional
            Tailscale overlay setup in `docs/operations/compose-deployment.md`.
          </p>
        )}
        {access.tailscale.enabled ? (
          <>
            {access.tailscale.httpsEndpoint ? (
              <p>
                Private HTTPS endpoint:{" "}
                <a
                  href={access.tailscale.httpsEndpoint}
                  target="_blank"
                  rel="noreferrer"
                >
                  {access.tailscale.httpsEndpoint}
                </a>
              </p>
            ) : (
              <p className="muted">
                No private HTTPS endpoint is currently configured.
              </p>
            )}
            {access.tailscale.error && (
              <p className="error">{access.tailscale.error}</p>
            )}
          </>
        ) : (
          <p className="muted">
            The local Office connection remains available. Core, Worker, and
            runtime services are not published by the Tailscale overlay.
          </p>
        )}
        <div className="ui-callout ui-callout--info access-remote-guidance">
          <strong>Tailscale HTTPS setup and privacy</strong>
          <p>
            Enable MagicDNS and HTTPS certificates in your tailnet settings.
            Tailscale certificate hostnames are published in public Certificate
            Transparency logs, so the device hostname can be visible publicly.
            The certificate does not publish Factory itself: Office stays
            private to the tailnet, and Funnel remains disabled.
          </p>
        </div>
      </Panel>

      <Panel>
        <div className="section-heading">
          <div>
            <h2>Enrolled devices</h2>
            <p className="muted">
              Each browser and phone receives its own session.
            </p>
          </div>
          <div className="row">
            <Button
              variant="primary"
              disabled={busy}
              onClick={() => void pairDevice()}
            >
              {busy ? "Preparing…" : "Pair device"}
            </Button>
            <Button
              variant="danger"
              disabled={busy}
              onClick={() => void revokeAll()}
            >
              Revoke all devices
            </Button>
          </div>
        </div>
        {challenge && qrValue && (
          <div className="access-pairing" aria-live="polite">
            <PairingQr value={qrValue} />
            <div>
              <h3>Scan with the phone camera</h3>
              <p>
                Confirm that this code matches on both screens before approving
                the pending device.
              </p>
              <dl className="detail">
                <dt>Verification code</dt>
                <dd>
                  <code className="access-code">
                    {challenge.verificationCode}
                  </code>
                </dd>
                <dt>Expires</dt>
                <dd>{formatTimestamp(challenge.expiresAt)}</dd>
              </dl>
              <p className="muted">
                The QR contains only a random challenge identifier. It grants no
                access. Approval is required here before Core creates a phone
                session.
              </p>
              <Button
                size="sm"
                disabled={busy}
                onClick={() => void cancelChallenge()}
              >
                Cancel pairing code
              </Button>
            </div>
          </div>
        )}
        {devices.length === 0 ? (
          <EmptyState
            title="No other devices enrolled"
            description="Pair a phone or another browser to create an independent session."
          />
        ) : (
          <ul className="access-device-list">
            {sortedDevices.map((device) => (
              <DeviceRow key={device.id} device={device} onChanged={refresh} />
            ))}
          </ul>
        )}
      </Panel>
      <Panel>
        <div className="section-heading">
          <div>
            <h2>Security activity</h2>
            <p className="muted">
              Recent session and pairing events recorded by Core.
            </p>
          </div>
        </div>
        {audit.length === 0 ? (
          <EmptyState
            title="No security events yet"
            description="Session creation, rotation, revocation and pairing decisions appear here."
          />
        ) : (
          <ul className="access-audit">
            {audit.slice(0, 20).map((entry) => (
              <li
                key={`${entry.occurredAt}-${entry.action}-${entry.deviceName ?? "unknown"}-${entry.actor}`}
              >
                <div>
                  <strong>{auditLabel(entry.action)}</strong>
                  <span>{entry.deviceName ?? "Unknown device"}</span>
                </div>
                <div className="access-audit-meta">
                  <span className="muted">by {entry.actor}</span>
                  <time>{formatTimestamp(entry.occurredAt)}</time>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Panel>
      <p className="muted access-refresh">
        Device list refreshes every five seconds. Last activity and expiry come
        from Core.
      </p>
    </div>
  );
}
