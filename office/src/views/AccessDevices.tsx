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
import { type MessageKey, type Translate, useI18n } from "../i18n";

const AUDIT_LABEL_KEYS: Record<string, MessageKey> = {
  created: "access.audit.created",
  rotated: "access.audit.rotated",
  revoked: "access.audit.revoked",
  revoked_all: "access.audit.revokedAll",
  claimed: "access.audit.claimed",
  approved: "access.audit.approved",
  rejected: "access.audit.rejected",
  cancelled: "access.audit.cancelled",
  consumed: "access.audit.consumed",
  revocation_requested: "access.audit.revocationRequested",
};

function auditLabel(action: string, t: Translate): string {
  const key = AUDIT_LABEL_KEYS[action];
  return key ? t(key) : action.replaceAll("_", " ");
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
  const { t } = useI18n();
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
      () => active && setError(t("access.pairing.qrError")),
    );
    return () => {
      active = false;
    };
  }, [t, value]);

  return (
    <div className="access-qr">
      <canvas ref={canvas} aria-label={t("access.pairing.qrAria")} />
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
  const { t, formatDate } = useI18n();
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
        cause instanceof CoreApiError
          ? cause.message
          : t("access.device.actionError"),
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
              {t("access.device.verifyCodeBefore")}{" "}
              <code className="access-code">
                {device.verificationCode ?? t("access.device.unavailable")}
              </code>{" "}
              {t("access.device.verifyCodeAfter")}
            </p>
            <label className="ui-field access-code-confirm">
              <span className="ui-field-label">
                {t("access.device.enterCode")}
              </span>
              <input
                className="ui-input"
                inputMode="numeric"
                autoComplete="one-time-code"
                value={confirmedCode}
                onChange={(event) => setConfirmedCode(event.target.value)}
                aria-label={t("access.device.confirmCodeAria")}
              />
            </label>
          </>
        ) : device.status === "APPROVED" ? (
          <p className="muted">{t("access.device.approvedWaiting")}</p>
        ) : (
          <p className="muted">
            {t("access.device.lastActivity")}{" "}
            {device.lastActivityAt
              ? formatDate(device.lastActivityAt)
              : t("common.notRecorded")}{" "}
            ·{" "}
            {t("access.device.expires", { date: formatDate(device.expiresAt) })}
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
              {t("access.device.approve")}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => void mutate(() => coreApi.rejectDevice(device.id))}
            >
              {t("access.device.reject")}
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
            {t("access.device.cancelApproval")}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="danger"
            disabled={busy}
            onClick={() => {
              if (device.current) {
                if (window.confirm(t("access.device.signOutConfirm"))) {
                  void mutate(async () => {
                    await logoutCurrentDevice();
                    window.location.reload();
                  });
                }
                return;
              }
              if (
                window.confirm(
                  t("access.device.revokeConfirm", { name: device.name }),
                )
              ) {
                void mutate(() => coreApi.revokeDevice(device.id));
              }
            }}
          >
            {device.current
              ? t("access.device.signOut")
              : t("access.device.revoke")}
          </Button>
        )}
      </div>
    </li>
  );
}

export function AccessDevices() {
  const { t, formatDate } = useI18n();
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
        cause instanceof CoreApiError ? cause.message : t("access.error.load"),
      );
      setLoading(false);
    }
  }, [t]);

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
      setNotice(t("access.pairing.createdNotice"));
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("access.pairing.createError"),
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
      setNotice(t("access.pairing.cancelledNotice"));
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("access.pairing.cancelError"),
      );
    } finally {
      setChallenge(null);
      setBusy(false);
    }
  }

  async function revokeAll() {
    if (!window.confirm(t("access.devices.revokeAllConfirm"))) return;
    setBusy(true);
    setError(null);
    try {
      await logoutAllDevices();
      window.location.reload();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("access.pairing.revokeAllError"),
      );
      setBusy(false);
    }
  }

  if (loading) return <LoadingState label={t("access.loading")} />;
  if (!access && error) {
    return (
      <ErrorState
        title={t("common.errorTitle")}
        message={error}
        action={
          <Button onClick={() => void refresh()}>{t("common.retry")}</Button>
        }
      />
    );
  }
  if (!access)
    return (
      <EmptyState
        title={t("access.error.unknownTitle")}
        description={t("access.error.unknownDescription")}
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
            <h2>{t("access.local.title")}</h2>
            <p className="muted">{t("access.local.description")}</p>
          </div>
          <div className="row">
            <span className="access-status">{access.local.status}</span>
            <Button size="sm" onClick={() => void refresh()}>
              {t("access.local.refresh")}
            </Button>
          </div>
        </div>
        <p className="muted">{t("access.local.sessionNote")}</p>
      </Panel>

      <Panel>
        <div className="section-heading">
          <h2>{t("access.remote.title")}</h2>
          <span
            className={`access-status access-status--${access.tailscale.status.toLowerCase()}`}
          >
            {access.tailscale.status}
          </span>
        </div>
        {access.tailscale.status === "CONNECTED" ? (
          <p className="muted">{t("access.remote.connected")}</p>
        ) : access.tailscale.status === "DISCONNECTED" ? (
          <p className="muted">{t("access.remote.disconnected")}</p>
        ) : access.tailscale.status === "UNKNOWN" ? (
          <p className="muted">{t("access.remote.unknown")}</p>
        ) : (
          <p className="muted">{t("access.remote.disabled")}</p>
        )}
        {access.tailscale.enabled ? (
          <>
            {access.tailscale.httpsEndpoint ? (
              <p>
                {t("access.remote.endpoint")}{" "}
                <a
                  href={access.tailscale.httpsEndpoint}
                  target="_blank"
                  rel="noreferrer"
                >
                  {access.tailscale.httpsEndpoint}
                </a>
              </p>
            ) : (
              <p className="muted">{t("access.remote.noEndpoint")}</p>
            )}
            {access.tailscale.error && (
              <p className="error">{access.tailscale.error}</p>
            )}
          </>
        ) : (
          <p className="muted">{t("access.remote.localAvailable")}</p>
        )}
        <div className="ui-callout ui-callout--info access-remote-guidance">
          <strong>{t("access.remote.guidanceTitle")}</strong>
          <p>{t("access.remote.guidance")}</p>
        </div>
      </Panel>

      <Panel>
        <div className="section-heading">
          <div>
            <h2>{t("access.devices.title")}</h2>
            <p className="muted">{t("access.devices.description")}</p>
          </div>
          <div className="row">
            <Button
              variant="primary"
              disabled={busy}
              onClick={() => void pairDevice()}
            >
              {busy ? t("access.devices.preparing") : t("access.devices.pair")}
            </Button>
            <Button
              variant="danger"
              disabled={busy}
              onClick={() => void revokeAll()}
            >
              {t("access.devices.revokeAll")}
            </Button>
          </div>
        </div>
        {challenge && qrValue && (
          <div className="access-pairing" aria-live="polite">
            <PairingQr value={qrValue} />
            <div>
              <h3>{t("access.pairing.scanTitle")}</h3>
              <p>{t("access.pairing.scanDescription")}</p>
              <dl className="detail">
                <dt>{t("access.pairing.verificationCode")}</dt>
                <dd>
                  <code className="access-code">
                    {challenge.verificationCode}
                  </code>
                </dd>
                <dt>{t("access.pairing.expires")}</dt>
                <dd>{formatDate(challenge.expiresAt)}</dd>
              </dl>
              <p className="muted">{t("access.pairing.qrNote")}</p>
              <Button
                size="sm"
                disabled={busy}
                onClick={() => void cancelChallenge()}
              >
                {t("access.pairing.cancel")}
              </Button>
            </div>
          </div>
        )}
        {devices.length === 0 ? (
          <EmptyState
            title={t("access.devices.emptyTitle")}
            description={t("access.devices.emptyDescription")}
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
            <h2>{t("access.audit.title")}</h2>
            <p className="muted">{t("access.audit.description")}</p>
          </div>
        </div>
        {audit.length === 0 ? (
          <EmptyState
            title={t("access.audit.emptyTitle")}
            description={t("access.audit.emptyDescription")}
          />
        ) : (
          <ul className="access-audit">
            {audit.slice(0, 20).map((entry) => (
              <li
                key={`${entry.occurredAt}-${entry.action}-${entry.deviceName ?? "unknown"}-${entry.actor}`}
              >
                <div>
                  <strong>{auditLabel(entry.action, t)}</strong>
                  <span>
                    {entry.deviceName ?? t("access.audit.unknownDevice")}
                  </span>
                </div>
                <div className="access-audit-meta">
                  <span className="muted">
                    {t("access.audit.by", { actor: entry.actor })}
                  </span>
                  <time>{formatDate(entry.occurredAt)}</time>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Panel>
      <p className="muted access-refresh">{t("access.devices.refreshNote")}</p>
    </div>
  );
}
