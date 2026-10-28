import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { Button } from "../components/ui/Button";
import {
  type AuthSession,
  CoreApiError,
  coreApi,
  type DeviceClaim,
  getAuthSession,
} from "../coreApi";
import { useI18n } from "../i18n";

export function DevicePairClaim({
  onAuthenticated,
}: {
  onAuthenticated?: (session: AuthSession) => void;
}) {
  const { t } = useI18n();
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const challengeId = searchParams.get("challengeId") ?? "";
  const [deviceName, setDeviceName] = useState("");
  const [claim, setClaim] = useState<DeviceClaim | null>(null);
  const [redirecting, setRedirecting] = useState(false);
  const redirectingRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useLayoutEffect(() => {
    if (!challengeId) return;
    const cleanHash = window.location.hash.split("?")[0];
    window.history.replaceState(
      null,
      "",
      `${window.location.pathname}${cleanHash}`,
    );
  }, [challengeId]);

  useEffect(() => {
    if (claim?.status !== "PENDING") return;
    let active = true;
    const timer = window.setInterval(() => {
      if (redirectingRef.current) return;
      void coreApi
        .pollDeviceClaim(challengeId, claim.pollToken)
        .then(async (result) => {
          if (!active) return;
          if (result.status === "APPROVED") {
            redirectingRef.current = true;
            setRedirecting(true);
            for (let attempt = 0; attempt < 10; attempt += 1) {
              const session = await getAuthSession().catch(() => null);
              if (!active) return;
              if (session?.authenticated) {
                onAuthenticated?.(session);
                navigate("/", { replace: true });
                return;
              }
              await new Promise((resolve) => setTimeout(resolve, 100));
            }
            window.location.reload();
            return;
          }
          if (result.status === "REJECTED") {
            setClaim((current) =>
              current ? { ...current, status: "REJECTED" } : current,
            );
          } else if (result.status === "EXPIRED") {
            setClaim((current) =>
              current ? { ...current, status: "EXPIRED" } : current,
            );
          } else if (result.status === "CANCELLED") {
            setClaim((current) =>
              current ? { ...current, status: "CANCELLED" } : current,
            );
          }
        })
        .catch((cause) => {
          if (active) {
            setError(
              cause instanceof CoreApiError
                ? cause.message
                : t("pair.errorStatus"),
            );
          }
        });
    }, 1500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [challengeId, claim, navigate, onAuthenticated, t]);

  const hasClaim = challengeId !== "";

  async function requestApproval(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const result = await coreApi.claimDevice(
        challengeId,
        deviceName.trim() || t("pair.defaultDeviceName"),
      );
      setClaim(result);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError ? cause.message : t("pair.errorClaim"),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="gate pair-gate">
      <div className="gate-brand">
        <span className="brand-mark" aria-hidden="true">
          SF
        </span>
        <span>
          <strong>Factory</strong>
          <span>Office</span>
        </span>
      </div>
      <h1>{t("pair.title")}</h1>
      {!hasClaim ? (
        <p className="muted">{t("pair.missingChallenge")}</p>
      ) : claim?.status === "PENDING" ? (
        <>
          <p>{t("pair.waitingOwner")}</p>
          <div className="pair-verification">
            <span className="muted">{t("pair.verificationCode")}</span>
            <strong className="access-code">{claim.verificationCode}</strong>
          </div>
          <p className="muted" role="status" aria-live="polite">
            {t("pair.waitingStatus")}
          </p>
        </>
      ) : redirecting || claim?.status === "APPROVED" ? (
        <p className="muted" role="status" aria-live="polite">
          {t("pair.approved")}
        </p>
      ) : claim?.status === "REJECTED" ? (
        <p className="error" role="alert">
          {t("pair.rejected")}
        </p>
      ) : claim?.status === "EXPIRED" ? (
        <p className="error" role="alert">
          {t("pair.expired")}
        </p>
      ) : claim?.status === "CANCELLED" ? (
        <p className="error" role="alert">
          {t("pair.cancelled")}
        </p>
      ) : (
        <>
          <p className="muted">{t("pair.confirmOnOwner")}</p>
          <form
            className="pair-form"
            onSubmit={(event) => void requestApproval(event)}
          >
            <label className="ui-field">
              <span className="ui-field-label">{t("pair.deviceName")}</span>
              <input
                className="ui-input"
                value={deviceName}
                onChange={(event) => setDeviceName(event.target.value)}
                autoComplete="off"
                maxLength={80}
                placeholder={t("pair.deviceNamePlaceholder")}
              />
            </label>
            <Button
              type="submit"
              variant="primary"
              disabled={busy || !hasClaim}
            >
              {busy ? t("pair.requesting") : t("pair.request")}
            </Button>
          </form>
        </>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <p className="muted">{t("pair.privacyNote")}</p>
      <Button variant="quiet" onClick={() => navigate("/")}>
        {t("pair.return")}
      </Button>
    </main>
  );
}
