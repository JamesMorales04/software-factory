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

export function DevicePairClaim({
  onAuthenticated,
}: {
  onAuthenticated?: (session: AuthSession) => void;
}) {
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
                : "Approval status could not be checked.",
            );
          }
        });
    }, 1500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [challengeId, claim, navigate, onAuthenticated]);

  const hasClaim = challengeId !== "";

  async function requestApproval(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const result = await coreApi.claimDevice(
        challengeId,
        deviceName.trim() || "Mobile device",
      );
      setClaim(result);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "This pairing challenge is invalid or expired.",
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
      <h1>Pair this device</h1>
      {!hasClaim ? (
        <p className="muted">
          This pairing link is missing its one-time challenge. Ask the owner to
          create a new QR code.
        </p>
      ) : claim?.status === "PENDING" ? (
        <>
          <p>
            Approval is waiting on the enrolled owner device. Verify that the
            code matches before approving.
          </p>
          <div className="pair-verification">
            <span className="muted">Verification code</span>
            <strong className="access-code">{claim.verificationCode}</strong>
          </div>
          <p className="muted" role="status" aria-live="polite">
            Waiting for owner approval. Keep this page open; this one-time
            request is not saved in browser storage.
          </p>
        </>
      ) : redirecting || claim?.status === "APPROVED" ? (
        <p className="muted" role="status" aria-live="polite">
          Approval received. Opening Factory Office…
        </p>
      ) : claim?.status === "REJECTED" ? (
        <p className="error" role="alert">
          The owner rejected this device request. Ask them to create a new
          pairing code.
        </p>
      ) : claim?.status === "EXPIRED" ? (
        <p className="error" role="alert">
          This device request expired. Ask the owner to create a new pairing
          code.
        </p>
      ) : claim?.status === "CANCELLED" ? (
        <p className="error" role="alert">
          The owner cancelled this pairing request. Ask them to create a new
          pairing code.
        </p>
      ) : (
        <>
          <p className="muted">
            Confirm this request on the owner’s Factory Office before the phone
            receives its own secure session.
          </p>
          <form
            className="pair-form"
            onSubmit={(event) => void requestApproval(event)}
          >
            <label className="ui-field">
              <span className="ui-field-label">Device name</span>
              <input
                className="ui-input"
                value={deviceName}
                onChange={(event) => setDeviceName(event.target.value)}
                autoComplete="off"
                maxLength={80}
                placeholder="My phone"
              />
            </label>
            <Button
              type="submit"
              variant="primary"
              disabled={busy || !hasClaim}
            >
              {busy ? "Requesting approval…" : "Request owner approval"}
            </Button>
          </form>
        </>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <p className="muted">
        The challenge identifier is removed from browser history before Office
        contacts Core. It grants no access; owner approval is required.
      </p>
      <Button variant="quiet" onClick={() => navigate("/")}>
        Return to Factory
      </Button>
    </main>
  );
}
